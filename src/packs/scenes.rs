//! Scenes: named fixture states a surface is drawn and tested in. A pack writes its own in
//! `scenes/*.luau`; the host adds the ones nobody thinks of (nothing, one, a thousand, long
//! titles, missing icons, still loading, failed, light, dark, the narrowest tile).

use std::path::Path;
use std::time::Duration;

use rune_luau::mlua::{Lua, Value as Lv};
use serde_json::{Map, Value, json};

use super::error::{PackError, Result};
use super::json::from_lua;
use super::runtime::Runtime;
use super::shared::{Request, Scene};
use super::types::Type;

/// Reads one scene file: Luau that returns a table. It runs in a bare, sandboxed VM with no
/// host functions at all, so a scene can compute its fixtures but reach nothing.
pub fn read(path: &Path) -> Result<Scene> {
    let name = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let at = format!("scenes/{name}.luau");
    let source = std::fs::read_to_string(path).map_err(|e| PackError::new("E0200", format!("can't read the scene: {e}")).at(at.clone()))?;
    let lua = Lua::new();
    let _ = lua.sandbox(true);
    let value: Lv = lua.load(&source).set_name(format!("={at}")).eval().map_err(|e| {
        let mut err = PackError::luau(&e.to_string());
        if err.at.is_empty() {
            err.at = at.clone();
        }
        err.learn = "echospot dev docs scenes".into();
        err
    })?;
    let json = from_lua(&value);
    if !json.is_object() && !json.as_array().is_some_and(Vec::is_empty) {
        return Err(PackError::new("E0202", "a scene returns a table: { load = { [\"github.prs\"] = { … } }, stores = { … } }").at(at).learn("echospot dev docs scenes"));
    }
    Ok(Scene::from_json(&name, &json))
}

/// A pack's own scenes, by file name.
pub fn of_pack(dir: &Path) -> Vec<Result<Scene>> {
    let mut files: Vec<_> = std::fs::read_dir(dir.join("scenes")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "luau")).collect();
    files.sort();
    files.iter().map(|p| read(p)).collect()
}

/// Every target of a pack that draws: `ability:<name>:panel`, `surface:<name>`, `board:<name>`.
pub fn targets(rt: &Runtime, pack: &str) -> Vec<String> {
    let Some(p) = rt.packs.get(pack) else { return Vec::new() };
    let mut out: Vec<String> = p.index.abilities.iter().filter(|(_, a)| a.faces.iter().any(|f| f == "panel") || a.has_run && !a.hidden).map(|(name, _)| format!("ability:{name}:panel")).collect();
    out.extend(p.index.surfaces.keys().map(|name| format!("surface:{name}")));
    out.extend(p.index.boards.keys().map(|name| format!("board:{name}")));
    out
}

/// A target from how an agent names it: `prs`, `github.prs`, `surface:timer`, `board:morning`,
/// `prs:tile`.
pub fn target(rt: &Runtime, pack: &str, name: &str) -> Result<String> {
    let all = targets(rt, pack);
    let name = name.strip_prefix(&format!("{pack}.")).unwrap_or(name);
    let Some(p) = rt.packs.get(pack) else { return Err(PackError::new("E0300", format!("no pack {pack:?}"))) };
    let wanted = if name.starts_with("ability:") || name.starts_with("surface:") || name.starts_with("board:") {
        name.to_string()
    } else if let Some((ability, face)) = name.split_once(':') {
        format!("ability:{ability}:{face}")
    } else if p.index.abilities.contains_key(name) {
        format!("ability:{name}:panel")
    } else if p.index.surfaces.contains_key(name) {
        format!("surface:{name}")
    } else if p.index.boards.contains_key(name) {
        format!("board:{name}")
    } else {
        String::new()
    };
    let known = wanted.strip_prefix("ability:").and_then(|rest| rest.split(':').next()).is_some_and(|a| p.index.abilities.contains_key(a))
        || all.contains(&wanted);
    if known {
        return Ok(wanted);
    }
    let names: Vec<String> = p.index.abilities.keys().chain(p.index.surfaces.keys()).chain(p.index.boards.keys()).cloned().collect();
    Err(PackError::new("E0300", format!("{pack} has no surface {name:?}")).nearest(name, names.iter().map(String::as_str)).learn("echospot dev scenes"))
}

/// A value of a type, made up: `long` stretches every text, `bare` leaves out what may be left out.
pub fn sample(ty: &Type, n: usize, long: bool, bare: bool) -> Value {
    let words = if long { "A very long title that goes on well past where any row could ever hope to show all of it without clipping, wrapping or pushing its neighbours away" } else { "Sample" };
    match ty.kind.as_str() {
        "text" | "markdown" | "code" | "any" => json!(format!("{words} {n}")),
        "number" | "duration" => json!(n as f64 * 7.0 + 1.0),
        "percent" => json!((n * 13 % 100) as f64),
        "bool" => json!(n % 2 == 0),
        "money" => json!({ "amount": 1234.5 * (n as f64 + 1.0), "currency": "USD" }),
        "quantity" => json!({ "value": 12.5 * (n as f64 + 1.0), "unit": "km" }),
        "currency" => json!("USD"),
        "url" => json!(format!("https://example.com/{}/{n}", if long { "a/very/long/path/that/keeps/going/and/going/past/any/reasonable/width" } else { "item" })),
        "file" | "image" => json!(format!("/home/user/{}{n}.png", if long { "A folder with a long name/and another inside it/" } else { "" })),
        "folder" => json!("/home/user/Projects"),
        "date" => json!("2026-11-16"),
        "time" => json!("09:30"),
        "datetime" => json!("2026-11-16T09:30:00+00:00"),
        "color" => json!("#ff9f0a"),
        "app" => json!("firefox"),
        "enum" => json!(ty.values.get(n % ty.values.len().max(1)).cloned().unwrap_or_default()),
        "event" => json!({ "title": format!("{words} {n}"), "start": "2026-11-16T09:30:00+00:00", "end": "2026-11-16T10:00:00+00:00", "location": if bare { Value::Null } else { json!("Room 4") } }),
        "window" => json!({ "address": format!("0x{n:x}"), "title": format!("{words} {n}"), "class": "firefox" }),
        "person" => json!({ "name": format!("{words} {n}"), "email": "person@example.com" }),
        "place" => json!({ "name": format!("{words} {n}") }),
        "table" => json!({ "columns": ["Name", "Value"], "rows": (0..4).map(|i| json!([format!("{words} {i}"), i])).collect::<Vec<_>>() }),
        "series" => json!((0..24).map(|i| json!({ "t": i, "v": ((i * 7 + n) % 13) as f64 })).collect::<Vec<_>>()),
        "list" => json!((0..3).map(|i| sample(ty.of.as_deref().unwrap_or(&Type::of("text")), i, long, bare)).collect::<Vec<_>>()),
        "record" => {
            let mut map = Map::new();
            for (k, t) in &ty.fields {
                // What may be left out, is: a missing icon, a missing subtitle.
                if bare && (t.optional || matches!(k.as_str(), "icon" | "image" | "subtitle" | "description")) {
                    continue;
                }
                map.insert(k.clone(), sample(t, n, long, bare));
            }
            Value::Object(map)
        }
        _ => json!(format!("{words} {n}")),
    }
}

fn many(of: &Type, count: usize, long: bool, bare: bool) -> Value {
    Value::Array((0..count).map(|i| sample(of, i, long, bare)).collect())
}

/// The addresses a target loads when it is drawn, found by drawing it once with nothing loaded.
pub fn loads(rt: &mut Runtime, pack: &str, target: &str) -> Vec<String> {
    let before = rt.shared.scene.borrow_mut().take();
    rt.shared.resources.borrow_mut().clear();
    // What the host loads for a face the pack didn't write, or for a board's tiles, is known
    // without drawing: it is held pending through the probe, so nothing real runs.
    let mut found: Vec<String> = Vec::new();
    let parts: Vec<&str> = target.splitn(3, ':').collect();
    if let Some(p) = rt.packs.get(pack) {
        match parts.as_slice() {
            ["ability", name, face] if p.ability(name).is_some_and(|a| !a.faces.iter().any(|f| f == face)) => found.push(format!("{pack}.{name}")),
            ["ability", name] if p.ability(name).is_some_and(|a| !a.faces.iter().any(|f| f == "panel")) => found.push(format!("{pack}.{name}")),
            ["board", name] => found.extend(p.index.boards.get(*name).into_iter().flat_map(|b| b.tiles.iter().map(|t| t.address.clone()))),
            _ => {}
        }
    }
    found.dedup();
    *rt.shared.scene.borrow_mut() = Some(Scene { name: "probe".into(), pending: found.iter().cloned().collect(), ..Default::default() });
    let _ = super::surface::render(rt, pack, target, &json!({}), &json!({}));
    // What it asked for is on the queue; none of it is run.
    let mut keep = Vec::new();
    while let Some(request) = rt.shared.take() {
        match request {
            Request::Load { address, .. } => {
                if !found.contains(&address) {
                    found.push(address);
                }
            }
            other => keep.push(other),
        }
    }
    for request in keep {
        rt.shared.push(request);
    }
    rt.shared.resources.borrow_mut().clear();
    *rt.shared.scene.borrow_mut() = before;
    found
}

/// The scenes the host makes up for a target, so an agent doesn't have to think of them.
pub fn stress(rt: &mut Runtime, pack: &str, target: &str) -> Vec<Scene> {
    let addresses = loads(rt, pack, target);
    let typed: Vec<(String, Type)> = addresses.iter().map(|a| (a.clone(), super::faces::output_type(rt, a).unwrap_or_else(|| Type::of("any")))).collect();
    let fixture = |make: &dyn Fn(&Type) -> Value| -> Map<String, Value> { typed.iter().map(|(a, t)| (a.clone(), make(t))).collect() };
    let of = |t: &Type| t.of.as_deref().cloned().unwrap_or_else(|| Type::of("text"));
    let listy = |t: &Type| t.kind == "list";
    let base = |name: &str, load: Map<String, Value>| Scene { name: name.into(), load, ..Default::default() };
    let mut out = vec![
        base("stress-empty", fixture(&|t| if listy(t) { json!([]) } else if t.kind == "any" { Value::Null } else { sample(t, 0, false, false) })),
        base("stress-one", fixture(&|t| if listy(t) { many(&of(t), 1, false, false) } else { sample(t, 0, false, false) })),
        base("stress-thousand", fixture(&|t| if listy(t) { many(&of(t), 1000, false, false) } else { sample(t, 999, false, false) })),
        base("stress-long-titles", fixture(&|t| if listy(t) { many(&of(t), 6, true, false) } else { sample(t, 0, true, false) })),
        base("stress-missing-icons", fixture(&|t| if listy(t) { many(&of(t), 6, false, true) } else { sample(t, 0, false, true) })),
    ];
    let usual = fixture(&|t| if listy(t) { many(&of(t), 6, false, false) } else { sample(t, 0, false, false) });
    out.push(Scene { name: "stress-slow-load".into(), pending: addresses.iter().cloned().collect(), ..Default::default() });
    out.push(Scene { name: "stress-error".into(), errors: addresses.iter().map(|a| (a.clone(), json!("The server didn't answer"))).collect(), ..Default::default() });
    out.push(Scene { name: "stress-light".into(), load: usual.clone(), dark: Some(false), ..Default::default() });
    out.push(Scene { name: "stress-dark".into(), load: usual.clone(), dark: Some(true), ..Default::default() });
    out.push(Scene { name: "stress-narrow".into(), load: usual, size: Some((super::surface::TILE, super::surface::TILE)), ..Default::default() });
    out
}

/// Draws a target in a scene and returns its blocks. With no scene the real abilities run, and
/// the draw is repeated until everything it loads has settled.
pub fn draw(rt: &mut Runtime, pack: &str, target: &str, scene: Option<Scene>) -> Result<Value> {
    let state = match &scene {
        Some(s) => json!({ "query": s.query, "dark": s.dark, "size": s.size.map(|(w, h)| json!([w, h])) }),
        None => json!({}),
    };
    let args = scene.as_ref().map(|s| s.args.clone()).filter(|a| !a.is_null()).unwrap_or_else(|| json!({}));
    let live = scene.is_none();
    rt.set_scene(scene);
    let mut blocks = super::surface::render(rt, pack, target, &args, &state);
    if live {
        for _ in 0..6 {
            rt.settle(Duration::from_secs(20));
            let next = super::surface::render(rt, pack, target, &args, &state);
            let same = matches!((&blocks, &next), (Ok(a), Ok(b)) if a == b);
            blocks = next;
            if same {
                break;
            }
        }
    }
    rt.set_scene(None);
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_fit_their_types() {
        let pr = Type { name: "pr".into(), ..Type::record([("title", Type::of("text")), ("url", Type::of("url")), ("icon", Type::of("text").optional()), ("failing", Type::of("bool"))]) };
        let list = Type::list(pr.clone());
        let made = many(&pr, 3, false, false);
        assert!(list.coerce(made.clone()).is_ok(), "{made}");
        assert!(sample(&pr, 0, true, false)["title"].as_str().unwrap().len() > 100);
        assert!(sample(&pr, 0, false, true).get("icon").is_none());
        for (kind, _) in crate::packs::types::CORE {
            let t = Type::of(kind);
            assert!(t.coerce(sample(&t, 1, false, false)).is_ok(), "{kind}");
        }
    }

    #[test]
    fn a_scene_file_is_plain_data() {
        let dir = std::env::temp_dir().join(format!("echospot-scene-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("scenes")).unwrap();
        std::fs::write(dir.join("scenes/failing.luau"), "return { load = { [\"github.prs\"] = { { title = \"Fix CI\", failing = true } } }, stores = { n = 3 }, pending = { \"x.y\" }, dark = false }").unwrap();
        std::fs::write(dir.join("scenes/broken.luau"), "return io.open('/etc/passwd')").unwrap();
        let scenes = of_pack(&dir);
        let failing = scenes.iter().flatten().find(|s| s.name == "failing").unwrap();
        assert_eq!(failing.load["github.prs"][0]["title"], json!("Fix CI"));
        assert_eq!((failing.stores["n"].clone(), failing.pending.contains("x.y"), failing.dark), (json!(3), true, Some(false)));
        assert!(scenes.iter().any(|s| s.is_err()), "a scene can't reach the disk");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
