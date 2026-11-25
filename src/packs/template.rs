//! `echospot dev new` and `echospot dev eject`: a pack that already loads, and a component's
//! own source to change. Templates are whole, working packs: the fastest way to learn the API
//! is to start from something that runs.

use std::path::{Path, PathBuf};

use super::error::{PackError, Result};
use super::manifest;

struct Template {
    name: &'static str,
    about: &'static str,
    init: &'static str,
    permissions: &'static str,
    tint: &'static str,
    /// Icons by name, from the shared sheet.
    icons: &'static [&'static str],
    test: &'static str,
    scene: Option<(&'static str, &'static str)>,
    intent: &'static str,
    /// A Rust pack (§20): its `src/lib.rs` and `Cargo.toml`. `init` is then its Luau UI, if any.
    rust: Option<(&'static str, &'static str)>,
}

const ICONS: &str = include_str!("templates/icons.txt");

const TEMPLATES: &[Template] = &[
    Template {
        name: "blank", about: "One ability that returns text", init: include_str!("templates/blank.luau"), permissions: "", tint: "blue",
        icons: &["hello"], test: include_str!("templates/blank_test.luau"), scene: None,
        intent: "A starting point: one read ability with an input, an output and an example.",
        rust: None,
    },
    Template {
        name: "currency", about: "A recipe with no permissions: wraps a free built-in", init: include_str!("templates/currency.luau"), permissions: "", tint: "green",
        icons: &["convert"], test: include_str!("templates/currency_test.luau"), scene: None,
        intent: "Convert between currencies from root search. It calls only calc.convert, so it needs no grant.",
        rust: None,
    },
    Template {
        name: "github", about: "An extension: network, a secret, a list with intents", init: include_str!("templates/github.luau"),
        permissions: "net = [\"api.github.com\"]\nsecrets = { github = [\"api.github.com\"] }\n", tint: "graphite",
        icons: &["prs"], test: include_str!("templates/github_test.luau"), scene: Some(("failing", include_str!("templates/github_scene.luau"))),
        intent: "List my open pull requests, with the failing ones standing out.\n\nNeeds a GitHub token stored as the secret \"github\" (EchoSpot → Packs → Set Secret), bound to api.github.com.",
        rust: None,
    },
    Template {
        name: "board", about: "A dashboard: tiles from this pack and from built-ins", init: include_str!("templates/board.luau"),
        permissions: "calls = [\"calendar.next\"]\n", tint: "indigo", icons: &["board"], test: "return {\n\t[\"knows the week\"] = function(t)\n\t\tt:ok(t:call(\"week\", {}) >= 1)\n\tend,\n}\n", scene: None,
        intent: "A board for the start of the day: the week number, days left in the year, the next event.",
        rust: None,
    },
    Template {
        name: "overlay", about: "A screen-corner overlay fed by a store and a trigger", init: include_str!("templates/overlay.luau"),
        permissions: "background = true\n", tint: "orange", icons: &["timer"],
        test: "return {\n\t[\"starting sets the countdown\"] = function(t)\n\t\tt:equal(t:call(\"start\", { minutes = 5 }), 5)\n\tend,\n\t[\"the overlay shows what is left\"] = function(t)\n\t\tt:store(\"remaining\", 90)\n\t\tt:match(t:tree(\"timer\"), \"1:30\")\n\tend,\n}\n",
        scene: Some(("running", "return { stores = { remaining = 1500 } }\n")),
        intent: "A focus timer in the corner of the screen while it runs.",
        rust: None,
    },
    Template {
        name: "rust", about: "The Rust tier: abilities compiled to WebAssembly, with a Luau panel", init: include_str!("templates/rust_init.luau"),
        permissions: "", tint: "teal", icons: &["prime"], test: include_str!("templates/rust_test.luau"), scene: None,
        intent: "Prime numbers worked out in Rust: the nth prime from root search, and a panel of the first ones.",
        rust: Some((include_str!("templates/rust_lib.rs"), include_str!("templates/rust_cargo.toml"))),
    },
];

pub fn names() -> Vec<(&'static str, &'static str)> {
    TEMPLATES.iter().map(|t| (t.name, t.about)).collect()
}

fn icon(name: &str) -> &'static str {
    ICONS.lines().find_map(|l| l.split_once('\t').filter(|(n, _)| *n == name).map(|(_, svg)| svg)).unwrap_or(super::icon::TEMPLATE)
}

/// A name for people from an id: `focus-timer` reads "Focus Timer".
pub fn title(id: &str) -> String {
    id.split('-').filter(|w| !w.is_empty()).map(crate::util::capitalize).collect::<Vec<_>>().join(" ")
}

/// The files of a new pack, by path.
pub fn files(id: &str, from: &str) -> Result<Vec<(String, String)>> {
    let Some(t) = TEMPLATES.iter().find(|t| t.name == from) else {
        return Err(PackError::new("E0412", format!("no template {from:?}")).nearest(from, TEMPLATES.iter().map(|t| t.name)).learn("echospot dev new <id> --from currency"));
    };
    let fill = |text: &str| text.replace("__ID__", id).replace("__TITLE__", &title(id));
    let permissions = if t.permissions.is_empty() { String::new() } else { format!("\n[permissions]\n{}", t.permissions) };
    let mut out = vec![
        ("pack.toml".to_string(), format!("id = \"{id}\"\nname = \"{}\"\napi = {}\ntint = \"{}\"\n{permissions}", title(id), super::API, t.tint)),
        ("init.luau".to_string(), fill(t.init)),
        (format!("tests/{}.luau", t.name), fill(t.test)),
        ("INTENT.md".to_string(), format!("# {}\n\n## What was asked for\n\n{}\n\n## Decisions\n\n- Started from the `{}` template.\n\n## Known limits\n\n- None yet.\n", title(id), t.intent, t.name)),
        (".luaurc".to_string(), super::defs::LUAURC.to_string()),
        (".gitignore".to_string(), format!(".echo/\nechospot.d.luau\ninbox/\n{}", if t.rust.is_some() { "target/\nCargo.lock\n" } else { "" })),
    ];
    if let Some((lib, cargo)) = t.rust {
        out.push(("Cargo.toml".to_string(), fill(cargo)));
        out.push(("src/lib.rs".to_string(), fill(lib)));
        out.push(("src/echo.rs".to_string(), super::wasm::ECHO_RS.to_string()));
        out.push(("wit/echospot.wit".to_string(), super::wasm::WIT.to_string()));
    }
    for name in t.icons {
        out.push((format!("icons/{name}.svg"), format!("{}\n", icon(name))));
    }
    if let Some((name, scene)) = t.scene {
        out.push((format!("scenes/{name}.luau"), fill(scene)));
    }
    Ok(out)
}

/// Writes a new pack into the user's packs folder and returns where.
pub fn create(id: &str, from: &str) -> Result<PathBuf> {
    if !manifest::valid_id(id) {
        return Err(PackError::new("E0102", format!("{id:?} isn't an id: lowercase letters, digits and dashes, starting with a letter")).learn("echospot dev docs pack.toml"));
    }
    if manifest::RESERVED.contains(&id) {
        return Err(PackError::new("E0102", format!("{id:?} is reserved for EchoSpot's own abilities")).learn("echospot dev docs pack.toml"));
    }
    let dir = super::packs_dir().join(id);
    if dir.exists() {
        return Err(PackError::new("E0102", format!("{} already exists", dir.display())).learn(format!("echospot dev check {id}")));
    }
    for (name, text) in files(id, from)? {
        let path = dir.join(&name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| PackError::new("E0200", format!("couldn't make {}: {e}", parent.display())))?;
        }
        std::fs::write(&path, text).map_err(|e| PackError::new("E0200", format!("couldn't write {}: {e}", path.display())))?;
    }
    Ok(dir)
}

/// Components whose Luau source can be copied into a pack and changed.
pub const EJECTABLE: &[(&str, &str)] = &[
    ("List", include_str!("luau/eject/List.luau")),
    ("Stat", include_str!("luau/eject/Stat.luau")),
    ("KeyValue", include_str!("luau/eject/KeyValue.luau")),
    ("Banner", include_str!("luau/eject/Banner.luau")),
    ("Empty", include_str!("luau/eject/Empty.luau")),
    ("Progress", include_str!("luau/eject/Progress.luau")),
    ("Grid", include_str!("luau/eject/Grid.luau")),
    ("Gallery", include_str!("luau/eject/Gallery.luau")),
    ("Timeline", include_str!("luau/eject/Timeline.luau")),
    ("Detail", include_str!("luau/eject/Detail.luau")),
    ("Chart", include_str!("luau/eject/Chart.luau")),
    ("Table", include_str!("luau/eject/Table.luau")),
    ("Tabs", include_str!("luau/eject/Tabs.luau")),
    ("Split", include_str!("luau/eject/Split.luau")),
    ("Stream", include_str!("luau/eject/Stream.luau")),
    ("Board", include_str!("luau/eject/Board.luau")),
    ("Form", include_str!("luau/eject/Form.luau")),
];

/// Components that stay the host's, and why. Every one can be ejected now.
const KEPT: &[(&str, &str)] = &[];

/// Copies a component's source to `ui/<Name>.luau` in a pack. It is written with public
/// primitives only, so everything about it can be changed.
pub fn eject(dir: &Path, component: &str) -> Result<PathBuf> {
    let Some((name, source)) = EJECTABLE.iter().find(|(n, _)| n.eq_ignore_ascii_case(component)) else {
        let known = super::blocks::COMPONENTS.iter().any(|(n, ..)| n.eq_ignore_ascii_case(component));
        let kept = KEPT.iter().find(|(n, _)| n.eq_ignore_ascii_case(component));
        let message = match kept {
            Some((name, why)) => format!("{name} can't be ejected: {why}"),
            None if known => format!("{component} can't be ejected; these can: {}", EJECTABLE.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")),
            None => format!("no component {component:?}"),
        };
        return Err(PackError::new("E0412", message).nearest(component, EJECTABLE.iter().map(|(n, _)| *n)).learn("echospot dev docs ui"));
    };
    let path = dir.join("ui").join(format!("{name}.luau"));
    if path.exists() {
        return Err(PackError::new("E0102", format!("{} already exists; it is yours to edit", path.display())));
    }
    std::fs::create_dir_all(dir.join("ui")).and_then(|()| std::fs::write(&path, source)).map_err(|e| PackError::new("E0200", format!("couldn't write {}: {e}", path.display())))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::runtime::{Runtime, State};
    use crate::packs::tests::isolate;

    fn write(dir: &Path, files: &[(String, String)]) {
        for (name, text) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    #[test]
    fn every_template_is_a_pack_that_loads_checks_and_passes_its_tests() {
        isolate();
        for (name, _) in names() {
            if name == "rust" && crate::packs::wasm::toolchain().is_err() {
                eprintln!("skipped the rust template: rustup target add {}", crate::packs::wasm::TARGET);
                continue;
            }
            let id = format!("tpl-{name}");
            let dir = std::env::temp_dir().join(format!("echospot-template-{}-{name}", std::process::id())).join(&id);
            let _ = std::fs::remove_dir_all(&dir);
            write(&dir, &files(&id, name).unwrap());
            let mut rt = Runtime::ephemeral();
            let report = crate::packs::check::check(&mut rt, &dir);
            assert!(report.errors.is_empty(), "{name}: {:#?}", report.errors);
            assert!(report.warnings.is_empty(), "{name}: {:#?}", report.warnings);
            assert!(matches!(rt.packs[&id].state, State::Ready | State::NeedsGrant), "{name}");
            // Nothing a template draws breaks a design rule, in any scene the host makes up.
            for drawn in crate::packs::check::design(&mut rt, &id, |_, _| None) {
                assert!(drawn.error.is_none(), "{name} {} in {}: {:?}", drawn.target, drawn.scene, drawn.error);
                assert!(!crate::packs::design::failed(&drawn.findings), "{name} {} in {}: {:#?}", drawn.target, drawn.scene, drawn.findings);
            }
            let tests = crate::packs::devtest::run(&dir, false, None);
            assert!(tests.ok && tests.passed > 0, "{name}: {:#?}", tests.cases);
            let _ = std::fs::remove_dir_all(dir.parent().unwrap());
        }
        assert_eq!(files("x", "nope").unwrap_err().nearest.len(), 0);
        assert_eq!(files("x", "curency").unwrap_err().nearest[0], "currency");
    }

    #[test]
    fn template_icons_follow_the_spec() {
        for line in ICONS.lines() {
            let (name, svg) = line.split_once('\t').unwrap();
            let report = crate::packs::icon::check(svg, None);
            assert!(report.ok, "{name}: {:#?}", report.problems);
        }
        assert_eq!(title("focus-timer"), "Focus Timer");
    }

    #[test]
    fn an_ejected_component_is_source_in_the_pack() {
        let dir = std::env::temp_dir().join(format!("echospot-eject-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = eject(&dir, "list").unwrap();
        assert!(path.ends_with("ui/List.luau") && std::fs::read_to_string(&path).unwrap().contains("ui.each"));
        assert_eq!(eject(&dir, "List").unwrap_err().code, "E0102");
        assert_eq!(KEPT.len(), 0, "every component can be ejected");
        assert_eq!(eject(&dir, "Lst").unwrap_err().nearest[0], "List");
        let lua = rune_luau::mlua::Lua::new();
        for (name, source) in EJECTABLE {
            lua.load(*source).set_name(*name).into_function().unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What each ejected component is drawn with in the test below: the props its own comment shows.
    fn sample(name: &str) -> &'static str {
        match name {
            "List" => r#"{ items = { { title = "One", url = "https://a.example" }, { title = "Two" } }, subtitle = "url", empty = "None" }"#,
            "Stat" => r#"{ value = 42, caption = "Answers" }"#,
            "KeyValue" => r#"{ items = { { "Branch", "main" }, { "Status", "green" } } }"#,
            "Banner" => r#"{ title = "Offline", text = "Showing saved data", intent = "muted" }"#,
            "Empty" => r#"{ title = "No Builds", hint = "Push a commit to see one" }"#,
            "Progress" => r#"{ value = 0.4, label = "Uploading" }"#,
            "Grid" => r#"{ items = { { name = "a" }, { name = "b" }, { name = "c" } }, title = "name", columns = 2, empty = "None", press = function(item, cx) cx:toast(item.name) end }"#,
            "Gallery" => r#"{ items = { { path = "a.png", name = "A" } }, image = "path", title = "name", empty = "None" }"#,
            "Timeline" => r#"{ items = { { title = "Build", at = "9:00" }, { title = "Deploy", at = "9:30" } }, time = "at", empty = "None" }"#,
            "Detail" => r##"{ markdown = "# Hello\n\nThere.", fields = { { "Repo", "rune" } } }"##,
            "Chart" => r#"{ series = { 1, 3, 2, 5 }, title = "Builds" }"#,
            "Table" => r#"{ columns = { "Repo", "Stars" }, rows = { { "rune", 12 }, { "echo", 7 } } }"#,
            "Tabs" => r#"{ tabs = { { title = "Open", content = ui.Empty { title = "None open" } }, { title = "Closed" } }, selected = 1, select = function(i, cx) cx:toast(tostring(i)) end }"#,
            "Split" => r#"{ left = ui.Empty { title = "Left" }, right = ui.Empty { title = "Right" }, ratio = 0.4 }"#,
            "Stream" => r#"{ source = "Some **text**" }"#,
            "Board" => r#"{ tiles = { { title = "Rate", content = ui.Stat { value = 1 }, size = "2x1" }, { content = ui.Stat { value = 2 }, size = "3x1" } } }"#,
            "Form" => r#"{ key = "note", fields = { title = require("@echo/types").text():describe("Title"), count = require("@echo/types").number():optional() }, submit = function(values, cx) cx:toast(values.title) end }"#,
            other => panic!("no sample for {other}"),
        }
    }

    #[test]
    fn every_ejected_component_draws_in_a_pack_without_breaking_a_design_rule() {
        isolate();
        for (name, _) in EJECTABLE {
            let id = format!("eject-{}", name.to_lowercase());
            let dir = std::env::temp_dir().join(format!("echospot-ejected-{}-{name}", std::process::id())).join(&id);
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            eject(&dir, name).unwrap();
            let init = format!(
                "local echo = require(\"@echo\")\nlocal ui = require(\"@echo/ui\")\nlocal C = require(\"@self/ui/{name}\")\nreturn echo.pack {{ surfaces = {{ demo = echo.panel {{ title = \"Demo\", render = function(view, cx) return C({}) end }} }} }}\n",
                sample(name),
            );
            write(&dir, &[("pack.toml".into(), format!("id = \"{id}\"\napi = 1\n")), ("init.luau".into(), init)]);
            let mut rt = Runtime::ephemeral();
            let report = crate::packs::check::check(&mut rt, &dir);
            assert!(report.errors.is_empty(), "{name}: {:#?}", report.errors);
            let drawn = crate::packs::check::design(&mut rt, &id, |_, _| None);
            assert!(!drawn.is_empty(), "{name}: nothing was drawn");
            for d in drawn {
                assert!(d.error.is_none(), "{name} in {}: {:?}", d.scene, d.error);
                assert!(!crate::packs::design::failed(&d.findings), "{name} in {}: {:#?}", d.scene, d.findings);
                assert!(!d.findings.iter().any(|f| f.rule == "raw-value" || f.rule == "unknown"), "{name} in {}: {:#?}", d.scene, d.findings);
            }
            let _ = std::fs::remove_dir_all(dir.parent().unwrap());
        }
    }
}
