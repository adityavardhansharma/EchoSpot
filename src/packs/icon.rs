//! Icons: every ability, verb, board and pack gets one, drawn by the agent in EchoSpot's style.
//! The spec limits form, not ideas, and this checks the form: the grid, the stroke, what an icon
//! may be made of, how heavy it reads, and that it isn't one EchoSpot already has.

use std::path::{Path, PathBuf};

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{self, roxmltree};
use serde::Serialize;

/// The grid every icon is drawn on, and the area the drawing stays inside.
pub const GRID: f32 = 24.0;
pub const SAFE: (f32, f32) = (2.0, 22.0);
pub const STROKE: f32 = 1.9;
/// A fill is only for a dot this many units across or less.
pub const DOT: f32 = 3.0;
pub const MAX_SHAPES: usize = 8;
/// Above this share of one icon's ink landing on another's, they are the same icon.
pub const TOO_SIMILAR: f32 = 0.86;
/// The sizes a sheet shows the icon at.
pub const SHEET_SIZES: &[u32] = &[16, 20, 24, 32, 76];

const SHAPES: &[&str] = &["path", "circle", "rect", "line", "polyline", "polygon", "ellipse"];

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub ok: bool,
    /// What breaks the spec; any of these fails the check.
    pub problems: Vec<String>,
    /// The share of the grid the icon inks, and the median over EchoSpot's own icons.
    pub weight: f32,
    pub median_weight: f32,
    /// The built-in it most resembles, and by how much (0 to 1).
    pub nearest: String,
    pub similarity: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<PathBuf>,
}

/// An SVG as resvg will draw it: `currentColor` becomes a real colour.
fn inked(svg: &str, color: &str) -> String {
    svg.replace("currentColor", color).replace("currentcolor", color)
}

/// A built-in glyph as a whole SVG.
fn builtin_svg(icon: &rune::Icon) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {v} {v}\">{}</svg>", icon.body, v = icon.view)
}

/// Draws an SVG into a square of `px` pixels, in `color`.
pub fn raster(svg: &str, px: u32, color: &str) -> Option<Pixmap> {
    let tree = usvg::Tree::from_str(&inked(svg, color), &usvg::Options::default()).ok()?;
    let mut pixmap = Pixmap::new(px, px)?;
    let size = tree.size();
    let scale = px as f32 / size.width().max(size.height()).max(1.0);
    resvg::render(&tree, Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Some(pixmap)
}

/// Which pixels hold ink.
fn mask(pixmap: &Pixmap) -> Vec<bool> {
    pixmap.pixels().iter().map(|p| p.alpha() > 96).collect()
}

fn coverage(mask: &[bool]) -> f32 {
    mask.iter().filter(|m| **m).count() as f32 / mask.len().max(1) as f32
}

/// A mask with every inked pixel spread to its neighbours, so two icons a pixel apart still meet.
fn spread(mask: &[bool], side: usize) -> Vec<bool> {
    let mut out = vec![false; mask.len()];
    for y in 0..side {
        for x in 0..side {
            if mask[y * side + x] {
                for (dx, dy) in [(0i32, 0i32), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < side && (ny as usize) < side {
                        out[ny as usize * side + nx as usize] = true;
                    }
                }
            }
        }
    }
    out
}

/// How alike two icons are: the smaller of the shares of each one's ink that lands on the other.
fn similarity(a: &[bool], b: &[bool], side: usize) -> f32 {
    let (wide_a, wide_b) = (spread(a, side), spread(b, side));
    let share = |ink: &[bool], on: &[bool]| {
        let total = ink.iter().filter(|m| **m).count().max(1) as f32;
        ink.iter().zip(on).filter(|(i, o)| **i && **o).count() as f32 / total
    };
    share(a, &wide_b).min(share(b, &wide_a))
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(|a, b| a.total_cmp(b));
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

fn number(node: roxmltree::Node, name: &str) -> Option<f32> {
    node.attribute(name)?.trim().trim_end_matches("px").parse().ok()
}

/// An attribute as the element sees it: its own, or the nearest ancestor's.
fn inherited<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.ancestors().find_map(|n| n.attribute(name))
}

/// How far across a filled shape is, when that can be read off it.
fn fill_extent(node: roxmltree::Node) -> Option<f32> {
    match node.tag_name().name() {
        "circle" => number(node, "r").map(|r| r * 2.0),
        "ellipse" => Some(number(node, "rx")?.max(number(node, "ry")?) * 2.0),
        "rect" => Some(number(node, "width")?.max(number(node, "height")?)),
        _ => None,
    }
}

/// Checks the form of an icon's SVG against the spec.
fn form(svg: &str, problems: &mut Vec<String>) {
    let doc = match roxmltree::Document::parse(svg) {
        Ok(doc) => doc,
        Err(e) => return problems.push(format!("not valid SVG: {e}")),
    };
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return problems.push("the root element must be <svg>".into());
    }
    let view: Vec<f32> = root.attribute("viewBox").unwrap_or("").split([' ', ',']).filter_map(|n| n.parse().ok()).collect();
    if view != [0.0, 0.0, GRID, GRID] {
        problems.push(format!("viewBox must be \"0 0 24 24\"; it is {:?}", root.attribute("viewBox").unwrap_or("missing")));
    }
    let mut shapes = 0;
    for node in doc.descendants().filter(|n| n.is_element()) {
        let tag = node.tag_name().name();
        match tag {
            "text" | "tspan" => problems.push("no text: an icon is a drawing".into()),
            "image" | "use" | "foreignObject" => problems.push(format!("no <{tag}>: draw with paths")),
            "linearGradient" | "radialGradient" | "pattern" | "filter" | "mask" | "clipPath" => problems.push(format!("no <{tag}>: one flat colour, currentColor")),
            _ => {}
        }
        if node.attribute("transform").is_some() {
            problems.push(format!("no transform on <{tag}>: draw it where it goes"));
        }
        if node.attribute("style").is_some() {
            problems.push(format!("no style attribute on <{tag}>: use stroke, fill and stroke-width attributes"));
        }
        if !SHAPES.contains(&tag) {
            continue;
        }
        shapes += 1;
        for attribute in ["stroke", "fill"] {
            if let Some(value) = inherited(node, attribute).filter(|v| !matches!(*v, "none" | "currentColor" | "currentcolor")) {
                problems.push(format!("{attribute}=\"{value}\" on <{tag}>: use currentColor, so the icon takes its tile's colour"));
            }
        }
        let filled = inherited(node, "fill").is_some_and(|f| f != "none") || inherited(node, "fill").is_none();
        let stroked = inherited(node, "stroke").is_some_and(|s| s != "none");
        if filled {
            match fill_extent(node) {
                Some(extent) if extent <= DOT => {}
                Some(extent) => problems.push(format!("a filled <{tag}> {extent} units across: fills are for dots of {DOT} units or less; draw the outline")),
                None if inherited(node, "fill").is_none() => problems.push(format!("<{tag}> has no fill attribute, so it fills black: add fill=\"none\" to the <svg>")),
                None => problems.push(format!("a filled <{tag}>: fills are for dots of {DOT} units or less; draw the outline")),
            }
        }
        if stroked {
            let width = inherited(node, "stroke-width").and_then(|w| w.parse::<f32>().ok()).unwrap_or(1.0);
            if (width - STROKE).abs() > 0.06 {
                problems.push(format!("stroke-width {width} on <{tag}>: EchoSpot's icons are drawn at {STROKE}"));
            }
            for (attribute, wanted) in [("stroke-linecap", "round"), ("stroke-linejoin", "round")] {
                if inherited(node, attribute) != Some(wanted) {
                    problems.push(format!("{attribute} must be \"{wanted}\" on <{tag}>"));
                }
            }
        } else if !filled {
            problems.push(format!("<{tag}> has neither stroke nor fill: it draws nothing"));
        }
    }
    if shapes == 0 {
        problems.push("the icon draws nothing: add a <path>".into());
    }
    if shapes > MAX_SHAPES {
        problems.push(format!("{shapes} shapes; at most {MAX_SHAPES} keep an icon readable at 16 pixels"));
    }
    problems.sort();
    problems.dedup();
}

/// Checks that the drawing stays inside the safe area, from where its ink actually lands.
fn safe_area(svg: &str, problems: &mut Vec<String>) {
    const PX: u32 = 240;
    let Some(pixmap) = raster(svg, PX, "#000000") else { return };
    let unit = PX as f32 / GRID;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (PX, PX, 0u32, 0u32);
    for (i, p) in pixmap.pixels().iter().enumerate() {
        if p.alpha() > 96 {
            let (x, y) = (i as u32 % PX, i as u32 / PX);
            (min_x, min_y, max_x, max_y) = (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y));
        }
    }
    if max_x < min_x {
        return;
    }
    let (left, top, right, bottom) = (min_x as f32 / unit, min_y as f32 / unit, (max_x + 1) as f32 / unit, (max_y + 1) as f32 / unit);
    // Half a stroke of grace: a line drawn on the edge of the safe area reaches a little past it.
    let grace = STROKE / 2.0 + 0.1;
    if left < SAFE.0 - grace || top < SAFE.0 - grace || right > SAFE.1 + grace || bottom > SAFE.1 + grace {
        problems.push(format!("the drawing spans {left:.1}–{right:.1} across and {top:.1}–{bottom:.1} down; keep it inside {}–{} on the 24 grid", SAFE.0, SAFE.1));
    }
    if right - left < 9.0 && bottom - top < 9.0 {
        problems.push(format!("the drawing is only {:.1} × {:.1} units; fill more of the 24 grid so it reads at 16 pixels", right - left, bottom - top));
    }
}

/// Checks an icon. `sheet` is where to write the comparison sheet, when one is wanted.
pub fn check(svg: &str, sheet: Option<&Path>) -> Report {
    let mut report = Report::default();
    form(svg, &mut report.problems);
    if report.problems.iter().any(|p| p.starts_with("not valid SVG")) {
        return report;
    }
    safe_area(svg, &mut report.problems);
    const SIDE: usize = 48;
    let Some(mine) = raster(svg, SIDE as u32, "#000000").map(|p| mask(&p)) else {
        report.problems.push("resvg couldn't draw it".into());
        return report;
    };
    report.weight = coverage(&mine);
    let builtins: Vec<(&str, Vec<bool>)> = crate::icons::all().into_iter().filter_map(|(name, icon)| Some((name, mask(&raster(&builtin_svg(icon), SIDE as u32, "#000000")?)))).collect();
    report.median_weight = median(builtins.iter().map(|(_, m)| coverage(m)).collect());
    if report.median_weight > 0.0 {
        let ratio = report.weight / report.median_weight;
        if ratio > 1.7 {
            report.problems.push(format!("too heavy: it inks {:.0}% of the grid, {ratio:.1}× the median of EchoSpot's icons; use fewer or shorter strokes", report.weight * 100.0));
        } else if ratio < 0.45 {
            report.problems.push(format!("too light: it inks {:.0}% of the grid, {ratio:.1}× the median of EchoSpot's icons; draw it larger", report.weight * 100.0));
        }
    }
    if let Some((name, score)) = builtins.iter().map(|(name, m)| (*name, similarity(&mine, m, SIDE))).max_by(|a, b| a.1.total_cmp(&b.1)) {
        report.nearest = name.to_lowercase();
        report.similarity = score;
        if score > TOO_SIMILAR {
            report.problems.push(format!("too close to EchoSpot's own \"{}\" icon ({:.0}% the same); draw the idea, not a generic shape", report.nearest, score * 100.0));
        }
    }
    if let Some(path) = sheet
        && write_sheet(svg, path).is_ok() {
        report.sheet = Some(path.to_path_buf());
    }
    report.ok = report.problems.is_empty();
    report
}

fn blit(sheet: &mut image::RgbaImage, pixmap: &Pixmap, x: u32, y: u32) {
    for (i, p) in pixmap.pixels().iter().enumerate() {
        let (px, py) = (x + i as u32 % pixmap.width(), y + i as u32 / pixmap.width());
        if p.alpha() == 0 || px >= sheet.width() || py >= sheet.height() {
            continue;
        }
        let under = *sheet.get_pixel(px, py);
        // tiny-skia holds premultiplied colour.
        let a = p.alpha() as f32 / 255.0;
        let mix = |top: u8, bottom: u8| (top as f32 + bottom as f32 * (1.0 - a)).min(255.0) as u8;
        sheet.put_pixel(px, py, image::Rgba([mix(p.red(), under[0]), mix(p.green(), under[1]), mix(p.blue(), under[2]), 255]));
    }
}

fn fill_rect(sheet: &mut image::RgbaImage, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    for py in y..(y + h).min(sheet.height()) {
        for px in x..(x + w).min(sheet.width()) {
            sheet.put_pixel(px, py, image::Rgba(color));
        }
    }
}

/// The sheet an agent looks at: the icon at every size, light and dark, on its tile, and beside
/// ten of EchoSpot's own.
pub fn write_sheet(svg: &str, path: &Path) -> Result<(), String> {
    const PAD: u32 = 24;
    const ROW: u32 = 76 + PAD * 2;
    let width = PAD + SHEET_SIZES.iter().map(|s| s + PAD).sum::<u32>() + (76 + PAD) + 11 * (32 + 12) + PAD;
    let mut sheet = image::RgbaImage::new(width, ROW * 2);
    for (band, (ground, ink)) in [([244u8, 244, 246, 255], "#1c1c1e"), ([28, 28, 30, 255], "#f2f2f7")].into_iter().enumerate() {
        let top = band as u32 * ROW;
        fill_rect(&mut sheet, 0, top, width, ROW, ground);
        let mut x = PAD;
        for size in SHEET_SIZES {
            if let Some(pixmap) = raster(svg, *size, ink) {
                blit(&mut sheet, &pixmap, x, top + (ROW - size) / 2);
            }
            x += size + PAD;
        }
        // On its tile: white on the tint, as a command's row shows it.
        fill_rect(&mut sheet, x, top + PAD, 76, 76, [48, 176, 199, 255]);
        if let Some(pixmap) = raster(svg, 48, "#ffffff") {
            blit(&mut sheet, &pixmap, x + 14, top + PAD + 14);
        }
        x += 76 + PAD;
        // Among its neighbours: it should look like it belongs.
        let neighbours: Vec<String> = crate::icons::all().into_iter().step_by((crate::icons::all().len() / 10).max(1)).take(10).map(|(_, icon)| builtin_svg(icon)).collect();
        for (i, neighbour) in neighbours.iter().enumerate() {
            let shown = if i == 5 { svg } else { neighbour.as_str() };
            if let Some(pixmap) = raster(shown, 32, ink) {
                blit(&mut sheet, &pixmap, x, top + (ROW - 32) / 2);
            }
            x += 32 + 12;
        }
    }
    sheet.save(path).map_err(|e| e.to_string())
}

/// A starting point that passes the check: the icon `echospot dev new` writes.
pub const TEMPLATE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M5 8.500h9.500a4.500 4.500 0 0 1 0 9H12"/><path d="M8 5.500l-3 3 3 3"/><path d="M5 19.500h4"/></svg>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn svg(inner: &str) -> String {
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.9\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{inner}</svg>")
    }

    #[test]
    fn the_template_follows_the_spec() {
        let report = check(TEMPLATE, None);
        assert!(report.ok, "{:#?}", report);
        assert!(report.weight > 0.0 && report.median_weight > 0.0 && !report.nearest.is_empty());
    }

    #[test]
    fn what_breaks_the_spec_is_named() {
        let has = |inner: &str, needle: &str| {
            let report = check(&svg(inner), None);
            assert!(report.problems.iter().any(|p| p.contains(needle)), "expected {needle:?} in {:#?}", report.problems);
        };
        has("<path d=\"M4 12h16\" stroke=\"#ff0000\"/>", "currentColor");
        has("<path d=\"M4 12h16\" stroke-width=\"3\"/>", "stroke-width 3");
        has("<text x=\"4\" y=\"12\">Hi</text><path d=\"M4 12h16M12 4v16\"/>", "no text");
        has("<rect x=\"4\" y=\"4\" width=\"16\" height=\"16\" fill=\"currentColor\"/>", "fills are for dots");
        has("<path d=\"M0 12h24M12 0v24\"/>", "keep it inside");
        has("<path d=\"M11 12h2\"/>", "only");
        has("<g transform=\"rotate(45)\"><path d=\"M4 12h16M12 4v16\"/></g>", "no transform");
        has(&"<path d=\"M4 12h16\"/>".repeat(9), "at most 8");
        let wrong_box = check("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.9\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M3 8h10\"/></svg>", None);
        assert!(wrong_box.problems.iter().any(|p| p.contains("viewBox")));
        assert!(!check("<svg", None).ok);
        // A dot may be filled.
        let dot = check(&svg("<path d=\"M5 6h14v12H5z\"/><circle cx=\"9\" cy=\"10\" r=\"1.2\" fill=\"currentColor\" stroke=\"none\"/><path d=\"M8 15h8\"/>"), None);
        assert!(!dot.problems.iter().any(|p| p.contains("fills are for dots")), "{:#?}", dot.problems);
    }

    #[test]
    fn a_copy_of_a_builtin_is_caught() {
        let search = builtin_svg(&crate::icons::SEARCH).replace("<svg ", "<svg fill=\"none\" ");
        let report = check(&search, None);
        assert_eq!(report.nearest, "search");
        assert!(report.similarity > TOO_SIMILAR && report.problems.iter().any(|p| p.contains("too close")), "{report:#?}");
    }

    #[test]
    fn the_sheet_is_written() {
        let path = std::env::temp_dir().join(format!("echospot-icon-sheet-{}.png", std::process::id()));
        let report = check(TEMPLATE, Some(&path));
        assert_eq!(report.sheet.as_deref(), Some(path.as_path()));
        assert!(std::fs::metadata(&path).unwrap().len() > 1000);
        let _ = std::fs::remove_file(&path);
    }
}
