//! The design checker. Agent-made UI fails in the same few ways: sizes and colours picked by
//! hand, no empty or error state, too many type sizes, text that doesn't fit, effects piled on.
//! This reads a surface's blocks, and the rectangles it was laid out to when it was rendered,
//! and says which of those happened, where, and what to write instead.

use serde::Serialize;
use serde_json::Value;

use super::blocks;
use super::error::nearest;
use super::looks::{self, Look};

/// The smallest thing a pointer should have to hit, in points.
pub const MIN_TARGET: f64 = 24.0;
/// WCAG AA: 4.5 to 1 for body text, 3 to 1 for large text.
pub const CONTRAST_AA: f64 = 4.5;
pub const CONTRAST_AA_LARGE: f64 = 3.0;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Finding {
    /// `raw-value`, `type-sizes`, `contrast`, `clipped`, `misaligned`, `tap-target`, `fx-budget`,
    /// `unknown`, `empty-state`.
    pub rule: &'static str,
    /// `error` fails the check; `warn` is worth fixing; `note` is a suggestion.
    pub severity: &'static str,
    /// Where in the tree: `List › row 2`.
    pub at: String,
    pub message: String,
    /// What to write instead.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fix: String,
    /// The rectangle it was found at, in points, when the surface was rendered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<[f64; 4]>,
}

fn finding(rule: &'static str, severity: &'static str, at: &str, message: String, fix: String) -> Finding {
    Finding { rule, severity, at: at.to_string(), message, fix, rect: None }
}

/// Props that are a pack saying *what*: nothing here is a size or a colour.
const INTERNAL: &[&str] = &["_ui", "state", "error", "type", "rows", "children", "child", "value", "text", "source", "name", "effects", "more", "code", "learn", "face", "run", "big", "sparkline", "cover", "clip", "caption", "subtitle", "image", "language", "address", "args", "hint", "content", "id", "size", "key"];

/// What each primitive reads beside the modifiers.
const PRIMITIVE_PROPS: &[&str] = &[
    "gap", "padding", "px", "py", "align", "justify", "grow", "width", "height", "fill", "color", "radius", "role", "weight", "wrap", "lines", "tint",
    "press", "payload", "change", "submit", "placeholder", "label", "kind", "series", "title", "border", "opacity", "columns",
];

fn is_hex(s: &str) -> bool {
    s.starts_with('#') && matches!(s.len(), 4 | 5 | 7 | 9) && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// sRGB in 0..1, with alpha, from `#rgb`, `#rrggbb` or `#rrggbbaa`.
pub fn parse_hex(s: &str) -> Option<[f64; 4]> {
    if !is_hex(s) {
        return None;
    }
    let digits: Vec<u32> = s[1..].chars().filter_map(|c| c.to_digit(16)).collect();
    let wide: Vec<u32> = if digits.len() <= 4 { digits.iter().map(|d| d * 17).collect() } else { digits.chunks(2).map(|c| c[0] * 16 + c[1]).collect() };
    Some([wide[0] as f64 / 255.0, wide[1] as f64 / 255.0, wide[2] as f64 / 255.0, wide.get(3).map_or(1.0, |a| *a as f64 / 255.0)])
}

fn over(top: [f64; 4], under: [f64; 4]) -> [f64; 4] {
    let a = top[3];
    [top[0] * a + under[0] * (1.0 - a), top[1] * a + under[1] * (1.0 - a), top[2] * a + under[2] * (1.0 - a), 1.0]
}

fn luminance(c: [f64; 4]) -> f64 {
    let lin = |v: f64| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// The WCAG contrast ratio of `text` drawn over `ground`.
pub fn contrast(text: [f64; 4], ground: [f64; 4]) -> f64 {
    let (a, b) = (luminance(over(text, ground)), luminance(ground));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn rgba(c: rune::Color) -> [f64; 4] {
    parse_hex(&c.hex()).unwrap_or([1.0, 1.0, 1.0, 1.0])
}

/// A colour a pack named, as this theme draws it: a token, a tint, or raw hex.
fn resolve(name: &str, dark: bool, look: &Look) -> Option<[f64; 4]> {
    if let Some(raw) = parse_hex(name) {
        return Some(raw);
    }
    let palette = look.palette(crate::theme::palette(dark));
    looks::colour(&palette, name).map(rgba)
}

/// The ground a surface stands on, with nothing behind the glass to help.
fn ground(dark: bool, look: &Look) -> [f64; 4] {
    match (look.name, dark) {
        ("ink", true) => [0.05, 0.05, 0.05, 1.0],
        ("ink", false) => [0.96, 0.95, 0.92, 1.0],
        (_, true) => [0.106, 0.106, 0.114, 1.0],
        (_, false) => [0.97, 0.97, 0.97, 1.0],
    }
}

/// A block's place in the tree, as a person would say it.
fn path(trail: &[String]) -> String {
    trail.join(" › ")
}

fn check_block(block: &Value, trail: &[String], look: &Look, sizes: &mut Vec<(String, String)>, fills: &[String], out: &mut Vec<Finding>) {
    let kind = blocks::kind(block);
    let at = path(trail);
    let Some(map) = block.as_object() else { return };
    let primitive = blocks::PRIMITIVES.iter().any(|(n, _)| *n == kind);
    let component = blocks::COMPONENTS.iter().find(|(n, _, _)| *n == kind);

    // Names: a typo in a prop is silently ignored when drawn, so it is said here.
    if let Some((_, _, props)) = component {
        let known: Vec<&str> = props.split(',').map(str::trim).chain(blocks::MODIFIERS.iter().map(|(n, _)| *n)).chain(INTERNAL.iter().copied())
            .chain(["heading", "title", "items", "actions", "columns", "unit", "trend", "history", "kind", "label", "icon", "tabs", "tiles", "left", "right", "values", "submit", "ability", "fields", "markdown", "placeholder", "filter", "detail", "tab"]).collect();
        for key in map.keys().filter(|k| !known.contains(&k.as_str())) {
            let near = nearest(key, known.iter().copied().filter(|k| !k.starts_with('_')));
            out.push(finding("unknown", "warn", &at, format!("{kind} has no prop {key:?}; it is ignored"), if near.is_empty() { format!("echospot dev api show {kind}") } else { format!("did you mean {}?", near.join(", ")) }));
        }
    }
    for (key, valid) in [("intent", blocks::INTENTS), ("look", looks::NAMES), ("density", &["compact", "regular", "roomy"][..])] {
        if let Some(value) = map.get(key).and_then(Value::as_str).filter(|v| !v.is_empty() && !valid.contains(v)) {
            out.push(finding("unknown", "error", &at, format!("no {key} {value:?}"), format!("use one of {}{}", valid.join(", "), nearest(value, valid.iter().copied()).first().map(|n| format!(" (did you mean {n}?)")).unwrap_or_default())));
        }
    }
    for (i, row) in blocks::items(block, "rows").iter().enumerate() {
        if let Some(value) = row.get("intent").and_then(Value::as_str).filter(|v| !v.is_empty() && !blocks::INTENTS.contains(v)) {
            out.push(finding("unknown", "error", &format!("{at} › row {}", i + 1), format!("no intent {value:?}"), format!("use one of {}", blocks::INTENTS.join(", "))));
        }
    }

    // Raw values where a token belongs. Only primitives take sizes and colours at all.
    if primitive {
        for (key, value) in map {
            match (key.as_str(), value) {
                ("color" | "fill" | "border" | "tint", Value::String(s)) if is_hex(s) => {
                    let names: Vec<&str> = looks::COLOURS.iter().copied().chain(super::manifest::TINTS.iter().copied()).collect();
                    out.push(finding("raw-value", "warn", &at, format!("{key} = {s:?} is a raw colour"), format!("name a token so it follows the theme and the Look: {}", names.join(", "))));
                }
                ("color" | "fill" | "border" | "tint", Value::String(s)) if resolve(s, true, look).is_none() => {
                    let names: Vec<&str> = looks::COLOURS.iter().copied().chain(super::manifest::TINTS.iter().copied()).collect();
                    out.push(finding("unknown", "error", &at, format!("no colour {s:?}"), format!("use {}{}", names.join(", "), nearest(s, names.iter().copied()).first().map(|n| format!(" (did you mean {n}?)")).unwrap_or_default())));
                }
                ("gap" | "padding" | "px" | "py", Value::Number(n)) => {
                    out.push(finding("raw-value", "warn", &at, format!("{key} = {n} is a raw size"), format!("name a space: {}", looks::SPACE.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "))));
                }
                ("gap" | "padding" | "px" | "py", Value::String(s)) if looks::space(s).is_none() => {
                    out.push(finding("unknown", "error", &at, format!("no space {s:?}"), format!("use {}", looks::SPACE.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "))));
                }
                ("radius", Value::Number(n)) => {
                    out.push(finding("raw-value", "warn", &at, format!("radius = {n} is a raw size"), format!("name a radius: {}", looks::RADIUS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "))));
                }
                ("radius", Value::String(s)) if looks::radius(s).is_none() => {
                    out.push(finding("unknown", "error", &at, format!("no radius {s:?}"), format!("use {}", looks::RADIUS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "))));
                }
                ("role", Value::String(s)) if looks::type_role(s).is_none() => {
                    let roles: Vec<&str> = looks::TYPE.iter().map(|(n, ..)| *n).collect();
                    out.push(finding("unknown", "error", &at, format!("no type role {s:?}"), format!("use {}", roles.join(", "))));
                }
                ("weight", Value::Number(n)) => out.push(finding("raw-value", "warn", &at, format!("weight = {n} is a raw weight"), "pick a type role instead: role = \"headline\"".into())),
                (k, _) if !PRIMITIVE_PROPS.contains(&k) && !INTERNAL.contains(&k) && !blocks::MODIFIERS.iter().any(|(n, _)| *n == k) => {
                    let near = nearest(k, PRIMITIVE_PROPS.iter().copied());
                    out.push(finding("unknown", "warn", &at, format!("ui.{kind} has no prop {k:?}; it is ignored"), if near.is_empty() { "echospot dev docs ui".into() } else { format!("did you mean {}?", near.join(", ")) }));
                }
                _ => {}
            }
        }
    }
    if kind == "text" {
        // The type sizes a pack chose itself; components bring their own, designed ones.
        match (map.get("size"), map.get("role").and_then(Value::as_str)) {
            (Some(Value::Number(n)), _) => {
                out.push(finding("raw-value", "warn", &at, format!("size = {n} is a raw type size"), format!("pick a type role: {}", looks::TYPE.iter().map(|(n, ..)| *n).collect::<Vec<_>>().join(", "))));
                sizes.push((n.to_string(), at.clone()));
            }
            (_, Some(role)) => sizes.push((looks::TYPE.iter().find(|(n, ..)| *n == role).map(|(_, size, ..)| size.to_string()).unwrap_or_else(|| role.to_string()), at.clone())),
            _ => sizes.push(("13".into(), at.clone())),
        }
        // Text a pack coloured itself must still be readable, in both themes.
        if let Some(color) = map.get("color").and_then(Value::as_str) {
            for dark in [true, false] {
                let base = ground(dark, look);
                let under = fills.iter().filter_map(|f| resolve(f, dark, look)).fold(base, |ground, fill| over(fill, ground));
                let Some(ink) = resolve(color, dark, look) else { continue };
                let large = map.get("role").and_then(Value::as_str).is_some_and(|r| matches!(r, "display" | "title"));
                let (ratio, need) = (contrast(ink, under), if large { CONTRAST_AA_LARGE } else { CONTRAST_AA });
                if ratio < need {
                    out.push(finding("contrast", "error", &at, format!("{color} text on its {} ground has contrast {ratio:.1}:1; WCAG AA needs {need}:1", if dark { "dark" } else { "light" }),
                        "use text, text-secondary or accent, which are chosen to read in both themes".into()));
                    break;
                }
            }
        }
    }
    // A press target a pack sized itself.
    if primitive && map.get("press").is_some() {
        for key in ["height", "width", "size"] {
            if let Some(n) = map.get(key).and_then(Value::as_f64).filter(|n| *n < MIN_TARGET) {
                out.push(finding("tap-target", "error", &at, format!("a pressable {kind} with {key} = {n} is under the {MIN_TARGET} point minimum"), format!("leave {key} out, or make it at least {MIN_TARGET}")));
            }
        }
    }

    // States every list should have an answer for.
    if matches!(kind, "List" | "Grid" | "Gallery" | "Timeline") && map.get("empty").is_none() {
        out.push(finding("empty-state", "note", &at, format!("{kind} has no `empty`; with nothing to show it says \"No Results\""), "say what is missing and what to do: empty = { title = \"No builds\", hint = \"Push a commit to see one\" }".into()));
    }
    // Text that has nowhere to go: these don't truncate, they crowd what is beside them.
    let long = |value: Option<&Value>, max: usize| value.map(|v| blocks::show(v, None)).filter(|s| s.chars().count() > max);
    if kind == "Stat"
        && let Some(text) = long(map.get("value").filter(|v| v.is_string()), 18) {
        out.push(finding("clipped", "warn", &at, format!("a Stat's value is one number or a few words; {:?} won't fit a tile", crate::util::truncate(&text, 40)), "put the long part in `caption`, or use ui.Detail".into()));
    }
    for (i, row) in blocks::items(block, "rows").iter().enumerate() {
        if let Some(text) = long(row.get("accessory").filter(|v| v.is_string()), 28) {
            out.push(finding("clipped", "warn", &format!("{at} › row {}", i + 1), format!("an accessory is a word or two at the trailing edge; {:?} pushes the title out", crate::util::truncate(&text, 40)), "move it to `subtitle`".into()));
            break;
        }
    }
    let actions = |v: Option<&Value>| v.and_then(Value::as_array).into_iter().flatten().filter_map(|a| a.get("title").and_then(Value::as_str)).filter(|t| t.chars().count() > 34).map(String::from).collect::<Vec<_>>();
    for title in actions(map.get("actions")).into_iter().chain(blocks::items(block, "rows").iter().take(1).flat_map(|r| actions(r.get("actions")))) {
        out.push(finding("clipped", "warn", &at, format!("the action title {title:?} is too long for the action bar"), "name the action in two or three words: \"Open in Browser\"".into()));
    }
    if kind == "Tabs" && blocks::items(block, "tabs").len() > 5 {
        out.push(finding("clipped", "warn", &at, format!("{} tabs don't fit across the palette", blocks::items(block, "tabs").len()), "keep to five tabs, or use a List with sections".into()));
    }
    if kind == "Table" && blocks::items(block, "columns").len() > 6 {
        out.push(finding("clipped", "warn", &at, format!("{} columns don't fit across the palette", blocks::items(block, "columns").len()), "keep to six columns; put the rest in a row's detail".into()));
    }

    // Down the tree, carrying the fills text will stand on.
    let mut fills = fills.to_vec();
    if let Some(fill) = map.get("fill").and_then(Value::as_str) {
        fills.push(fill.to_string());
    }
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for child in blocks::children(block) {
        let name = blocks::kind(child);
        let n = counts.entry(name).or_insert(0);
        *n += 1;
        let mut next = trail.to_vec();
        next.push(if *n > 1 { format!("{name} {n}") } else { name.to_string() });
        check_block(child, &next, look, sizes, &fills, out);
    }
}

/// Checks a surface's blocks: what can be known without drawing it.
pub fn check(root: &Value, look: &Look) -> Vec<Finding> {
    let mut out = Vec::new();
    if root.get("_ui").is_none() {
        if !root.is_null() {
            out.push(finding("unknown", "error", "", "the surface didn't return a block".into(), "return a component from @echo/ui: return ui.List { … }".into()));
        }
        return out;
    }
    let mut sizes = Vec::new();
    check_block(root, &[blocks::kind(root).to_string()], look, &mut sizes, &[], &mut out);
    let mut distinct: Vec<&str> = sizes.iter().map(|(s, _)| s.as_str()).collect();
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() > 3 {
        out.push(finding("type-sizes", "error", &sizes.last().map(|(_, at)| at.clone()).unwrap_or_default(), format!("{} type sizes on one surface ({}); more than three reads as noise", distinct.len(), distinct.join(", ")),
            "keep to three roles: one for what matters, body, and caption".into()));
    }
    let (layers, errors) = super::fx::layers(root, look, look.fx_budget);
    for e in errors {
        out.push(finding("fx-budget", "error", &e.at, e.message.clone(), if e.nearest.is_empty() { e.learn.clone() } else { format!("did you mean {}?", e.nearest.join(", ")) }));
    }
    let frozen: Vec<String> = layers.iter().filter(|l| l.frozen).map(|l| format!("{} at {}", l.name, l.at)).collect();
    if !frozen.is_empty() {
        out.push(finding("fx-budget", "warn", &layers[0].at, format!("{} effect layers, and the {} Look runs {} at once; these hold still instead: {}", layers.len(), look.name, look.fx_budget, frozen.join("; ")),
            "mark fewer things urgent or live, so the ones that are stand out".into()));
    }
    out
}

/// One laid-out element, from a headless render's tree.
#[derive(Clone, Debug)]
struct Laid {
    path: String,
    rect: [f64; 4],
    role: String,
    text: String,
}

fn laid(tree: &Value) -> Vec<Laid> {
    tree.get("elements").and_then(Value::as_array).into_iter().flatten().filter_map(|e| {
        let rect = e.get("rect")?.as_array()?;
        Some(Laid {
            path: e.get("path")?.as_str()?.to_string(),
            rect: [rect.first()?.as_f64()?, rect.get(1)?.as_f64()?, rect.get(2)?.as_f64()?, rect.get(3)?.as_f64()?],
            role: e.get("role").and_then(Value::as_str).unwrap_or("").to_string(),
            text: e.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
        })
    }).collect()
}

/// Checks what only a layout knows: where every rectangle landed. `tree` is what a headless
/// render wrote with `--tree`.
pub fn measure(tree: &Value) -> Vec<Finding> {
    let all = laid(tree);
    let mut out = Vec::new();
    let Some(panel) = all.iter().find(|e| e.path.ends_with("/panel")).map(|e| e.rect) else { return out };
    // The pack's own part of the palette: not the header, the bottom bar or a menu.
    let body: Vec<&Laid> = all.iter().filter(|e| e.path.contains("/panel/") && !["/header", "/bottom", "/menu", "/dialog"].iter().any(|k| e.path.contains(k)) && e.rect[2] > 0.0 && e.rect[3] > 0.0).collect();
    let (right, bottom) = (panel[0] + panel[2], panel[1] + panel[3]);
    for e in body.iter().filter(|e| e.role == "text" && !e.text.is_empty()) {
        // Below the panel is a scroll away; past its side edge is lost.
        if e.rect[0] + e.rect[2] > right + 1.0 || e.rect[0] < panel[0] - 1.0 {
            let mut f = finding("clipped", "error", &e.path, format!("{:?} runs past the edge of the palette", crate::util::truncate(&e.text, 40)), "let the component lay it out: give long text to `subtitle`, `markdown` or a Detail".into());
            f.rect = Some(e.rect);
            out.push(f);
        }
    }
    // Left edges that are nearly, but not quite, the same.
    let mut lefts: Vec<(f64, &Laid)> = body.iter().filter(|e| e.role == "text" && !e.text.is_empty() && e.rect[1] < bottom).map(|e| (e.rect[0], *e)).collect();
    lefts.sort_by(|a, b| a.0.total_cmp(&b.0));
    for pair in lefts.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        if gap > 0.01 && gap <= 1.0 {
            let mut f = finding("misaligned", "warn", &pair[1].1.path, format!("{:?} starts {gap:.2} points to the right of {:?}; edges this close should be the same", crate::util::truncate(&pair[1].1.text, 30), crate::util::truncate(&pair[0].1.text, 30)),
                "use the same padding token on both, or put them in one ui.col".into());
            f.rect = Some(pair[1].1.rect);
            out.push(f);
            break;
        }
    }
    for e in body.iter().filter(|e| e.role == "button" && e.path.contains("/pack-")) {
        if e.rect[2].min(e.rect[3]) < MIN_TARGET {
            let mut f = finding("tap-target", "error", &e.path, format!("a press target {:.0} × {:.0} points is under the {MIN_TARGET} point minimum", e.rect[2], e.rect[3]), "let the component size it, or give it more padding".into());
            f.rect = Some(e.rect);
            out.push(f);
        }
    }
    out
}

/// Draws a box around each finding that has a rectangle, on the PNG the surface was rendered to.
pub fn mark(png: &std::path::Path, findings: &[Finding], scale: f64) -> Result<(), String> {
    let boxed: Vec<&Finding> = findings.iter().filter(|f| f.rect.is_some()).collect();
    if boxed.is_empty() {
        return Ok(());
    }
    let mut image = image::open(png).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = (image.width() as i64, image.height() as i64);
    for f in boxed {
        let [x, y, rw, rh] = f.rect.expect("filtered");
        let color = image::Rgba(if f.severity == "error" { [255, 69, 58, 255] } else { [255, 159, 10, 255] });
        let (x0, y0, x1, y1) = (((x - 2.0) * scale) as i64, ((y - 2.0) * scale) as i64, ((x + rw + 2.0) * scale) as i64, ((y + rh + 2.0) * scale) as i64);
        for thickness in 0..(2.0 * scale).max(2.0) as i64 {
            for px in x0..=x1 {
                for py in [y0 + thickness, y1 - thickness] {
                    if (0..w).contains(&px) && (0..h).contains(&py) {
                        image.put_pixel(px as u32, py as u32, color);
                    }
                }
            }
            for py in y0..=y1 {
                for px in [x0 + thickness, x1 - thickness] {
                    if (0..w).contains(&px) && (0..h).contains(&py) {
                        image.put_pixel(px as u32, py as u32, color);
                    }
                }
            }
        }
    }
    image.save(png).map_err(|e| e.to_string())
}

/// Whether a list of findings fails the check.
pub fn failed(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.severity == "error")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rules(findings: &[Finding]) -> Vec<&'static str> {
        findings.iter().map(|f| f.rule).collect()
    }

    #[test]
    fn components_pass_and_hand_picked_values_dont() {
        let look = looks::get("glass");
        let good = json!({ "_ui": "List", "state": "ok", "rows": [{ "id": "1", "title": "Fix CI", "intent": "urgent" }], "empty": "No open pull requests" });
        assert!(check(&good, look).is_empty(), "{:?}", check(&good, look));
        let raw = json!({ "_ui": "col", "gap": 7, "children": [
            { "_ui": "text", "text": "Hi", "color": "#333333", "size": 15 },
            { "_ui": "text", "text": "a", "role": "display" }, { "_ui": "text", "text": "b", "role": "caption" }, { "_ui": "text", "text": "c", "role": "title" },
            { "_ui": "box", "press": { "_handler": 1 }, "height": 16, "radius": "pill", "children": [] },
        ] });
        let found = check(&raw, look);
        let r = rules(&found);
        assert!(r.contains(&"raw-value") && r.contains(&"contrast") && r.contains(&"type-sizes") && r.contains(&"tap-target") && r.contains(&"unknown"), "{found:#?}");
        assert!(failed(&found));
        assert_eq!(found.iter().find(|f| f.rule == "contrast").unwrap().at, "col › text");
    }

    #[test]
    fn names_that_arent_names_are_said() {
        let look = looks::get("glass");
        let typo = json!({ "_ui": "List", "rows": [{ "title": "x", "intent": "urgnt" }], "empty": "None", "subtitel": "repo", "density": "tight" });
        let found = check(&typo, look);
        assert!(found.iter().any(|f| f.message.contains("subtitel") && f.fix.contains("subtitle")), "{found:#?}");
        assert!(found.iter().any(|f| f.message.contains("urgnt")) && found.iter().any(|f| f.message.contains("tight")));
        let none = json!({ "_ui": "List", "rows": [] });
        assert_eq!(rules(&check(&none, look)), vec!["empty-state"]);
        assert!(!failed(&check(&none, look)));
    }

    #[test]
    fn contrast_is_the_wcag_ratio() {
        assert!((contrast([0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]) - 21.0).abs() < 0.01);
        assert!(contrast([1.0, 1.0, 1.0, 0.3], [0.1, 0.1, 0.1, 1.0]) < 4.5);
        assert_eq!(parse_hex("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_hex("#00000080").map(|c| (c[3] * 100.0).round()), Some(50.0));
    }

    #[test]
    fn a_layout_shows_what_ran_off_and_what_nearly_lines_up() {
        let tree = json!({ "elements": [
            { "path": "#0/panel", "rect": [44.0, 44.0, 750.0, 475.0], "role": "group" },
            { "path": "#0/panel/pack-body/#0", "rect": [60.0, 100.0, 900.0, 18.0], "role": "text", "text": "A title that never ends" },
            { "path": "#0/panel/pack-body/#1", "rect": [60.5, 130.0, 80.0, 18.0], "role": "text", "text": "Nearly aligned" },
            { "path": "#0/panel/pack-body/#2", "rect": [60.0, 160.0, 14.0, 14.0], "role": "button" },
            { "path": "#0/panel/header/#0", "rect": [60.2, 60.0, 80.0, 18.0], "role": "text", "text": "Search" },
        ] });
        let found = measure(&tree);
        assert_eq!(rules(&found), vec!["clipped", "misaligned", "tap-target"], "{found:#?}");
        assert_eq!(found[0].rect, Some([60.0, 100.0, 900.0, 18.0]));
    }
}
