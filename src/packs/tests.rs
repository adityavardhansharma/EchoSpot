//! The runtime, end to end: real packs in a temporary folder, a real VM each, the real broker.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use super::runtime::{Event, Origin, Responder, Runtime, State};

/// A scratch state folder shared by every test in the process, so none touches the real one.
pub fn isolate() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("echospot-packs-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // Safety: set once, before any test thread reads it.
        unsafe { std::env::set_var("ECHOSPOT_PACKS_STATE", &dir) };
        // Rust packs' builds share one folder that outlives the run, so only the first builds the crates they use.
        // Safety: as above.
        unsafe { std::env::set_var("ECHOSPOT_RUST_TARGET", std::env::temp_dir().join("echospot-test-rust-target")) };
        crate::features::calc::currency::set_test_rates();
    });
}

pub struct Fixture {
    pub root: PathBuf,
    pub rt: Runtime,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Fixture {
    pub fn new(name: &str) -> Self {
        isolate();
        let root = std::env::temp_dir().join(format!("echospot-packs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self { root, rt: Runtime::ephemeral() }
    }

    pub fn dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// Writes a pack and loads it. `perms` is the body of `[permissions]`.
    pub fn pack(&mut self, id: &str, perms: &str, init: &str) -> &mut Self {
        self.write(id, &[("pack.toml", &format!("id = \"{id}\"\napi = 1\n[permissions]\n{perms}\n")), ("init.luau", init)]);
        self.load(id)
    }

    pub fn write(&mut self, id: &str, files: &[(&str, &str)]) -> &mut Self {
        for (name, text) in files {
            let path = self.dir(id).join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        self
    }

    pub fn load(&mut self, id: &str) -> &mut Self {
        let dir = self.dir(id);
        self.rt.load(&dir, true);
        self
    }

    pub fn call(&mut self, address: &str, input: Value) -> Result<Value, super::error::PackError> {
        self.rt.call(address, input, false)
    }

    pub fn state(&self, id: &str) -> State {
        self.rt.packs[id].state
    }
}

const HEAD: &str = "local echo = require(\"@echo\")\nlocal t = require(\"@echo/types\")\nlocal ui = require(\"@echo/ui\")\n";

fn pack(body: &str) -> String {
    format!("{HEAD}return echo.pack {{ {body} }}")
}

#[test]
fn the_currency_recipe_runs_with_no_grant() {
    let mut f = Fixture::new("currency");
    f.pack("currency", "", &pack(r#"abilities = { convert = echo.ability {
        title = "Convert Currency", kind = "read",
        input = { amount = t.number(), from = t.currency(), to = t.currency() }, output = t.money(),
        run = function(input, cx) return cx:call("calc.convert", input) end,
    } }"#));
    assert_eq!(f.state("currency"), State::Ready);
    let out = f.call("currency.convert", json!({ "amount": "50", "from": "usd", "to": "inr" })).unwrap();
    assert_eq!(out["currency"], json!("INR"));
    assert!(out["amount"].as_f64().unwrap() > 50.0, "{out}");
    let e = f.call("currency.convert", json!({ "amount": "fifty", "from": "usd", "to": "inr" })).unwrap_err();
    assert_eq!((e.code, e.learn.as_str()), ("E0301", "echospot dev api show currency.convert"), "{e}");
    let e = f.call("currency.convrt", json!({})).unwrap_err();
    assert_eq!((e.code, e.nearest[0].as_str()), ("E0300", "currency.convert"), "{e}");
}

#[test]
fn a_read_cannot_reach_an_effect() {
    let mut f = Fixture::new("kinds");
    f.pack("k", "", &pack(r#"abilities = {
        peek = echo.ability { kind = "read", run = function(_, cx) return cx:call("k.poke", {}) end },
        poke = echo.ability { kind = "effect", run = function(_, cx) cx:call("system.notify", { title = "hi" }) return 1 end },
        deep = echo.ability { kind = "read", run = function(_, cx) return cx:call("k.shallow", {}) end },
        shallow = echo.ability { kind = "read", run = function(_, cx) local ok, err = cx:try("system.notify", { title = "x" }) return { ok = ok, code = err.code } end },
        send = echo.ability { kind = "final", run = function() return "sent" end },
        sloppy = echo.ability { kind = "effect", run = function(_, cx) return cx:call("k.send", {}) end },
        careful = echo.ability { kind = "effect", undo = function() end, run = function(_, cx) return cx:call("k.send", {}) end },
    }"#));
    let e = f.call("k.peek", json!({})).unwrap_err();
    assert_eq!(e.code, "E0310", "{e}");
    assert!(e.message.contains("k.peek") && e.message.contains("k.poke"), "{e}");
    // The rule holds through any number of calls, and `try` reports it without raising.
    assert_eq!(f.call("k.deep", json!({})).unwrap(), json!({ "ok": false, "code": "E0310" }));
    assert_eq!(f.call("k.sloppy", json!({})).unwrap_err().code, "E0311");
    assert_eq!(f.call("k.careful", json!({})).unwrap(), json!("sent"));
}

#[test]
fn another_pack_is_reached_only_through_calls_that_were_granted() {
    let mut f = Fixture::new("calls");
    f.pack("lib", "", &pack(r#"abilities = { pi = echo.ability { kind = "read", output = t.number(), run = function() return 3.14 end } }"#));
    let user = pack(r#"abilities = { area = echo.ability { kind = "read", input = { r = t.number() }, run = function(i, cx) return cx:call("lib.pi", {}) * i.r * i.r end } }"#);
    f.pack("geo", "", &user);
    let e = f.call("geo.area", json!({ "r": 2 })).unwrap_err();
    assert_eq!(e.code, "E0401", "{e}");
    assert!(e.message.contains("calls = [\"lib.pi\"]"), "{e}");
    // Declared but not granted: the pack waits for a person.
    f.pack("geo", "calls = [\"lib.pi\"]", &user);
    assert_eq!(f.state("geo"), State::NeedsGrant);
    assert_eq!(f.call("geo.area", json!({ "r": 2 })).unwrap_err().code, "E0400");
    f.rt.grant_all("geo");
    assert_eq!(f.state("geo"), State::Ready);
    assert_eq!(f.call("geo.area", json!({ "r": 2 })).unwrap(), json!(12.56));
    f.rt.revoke("geo", None);
    assert_eq!(f.state("geo"), State::NeedsGrant);
}

#[test]
fn effects_are_recorded_stubbed_and_undone() {
    let mut f = Fixture::new("undo");
    f.pack("todo", "", &pack(r#"
        stores = { items = echo.store { type = t.list(t.text()), initial = {} } },
        abilities = {
            list = echo.ability { kind = "read", output = t.list(t.text()), run = function(_, cx) return cx:store("items") end },
            add = echo.ability { title = "Add Item", kind = "effect", input = { text = t.text() },
                run = function(i, cx) local items = table.clone(cx:store("items")) table.insert(items, i.text) cx:put("items", items) return #items end,
                undo = function(i, n, cx) local items = table.clone(cx:store("items")) table.remove(items, n) cx:put("items", items) end },
            ping = echo.ability { kind = "effect", run = function(_, cx) cx:call("system.notify", { title = "ping" }) return cx:dry() end },
        }"#));
    assert_eq!(f.call("todo.add", json!({ "text": "milk" })).unwrap(), json!(1));
    assert_eq!(f.call("todo.add", json!({ "text": "eggs" })).unwrap(), json!(2));
    assert_eq!(f.call("todo.list", json!({})).unwrap(), json!(["milk", "eggs"]));
    let effect = f.rt.timeline.back().unwrap().clone();
    assert_eq!((effect.title.as_str(), effect.summary.as_str(), f.rt.timeline.len()), ("Add Item", "{\"text\":\"eggs\"}", 2));
    assert!(f.rt.events.contains(&Event::Undoable { id: effect.id, title: "Add Item".into() }));
    for token in f.rt.undo(effect.id).unwrap() {
        f.rt.wait(token, Duration::from_secs(5)).unwrap();
    }
    assert_eq!(f.call("todo.list", json!({})).unwrap(), json!(["milk"]));
    assert_eq!(f.rt.undo(effect.id).unwrap_err().code, "E0311");
    // A dry run runs the ability and lists what it would have done.
    assert_eq!(f.rt.call("todo.ping", json!({}), true).unwrap(), json!(true));
    assert_eq!(f.rt.dry_log.len(), 1);
    assert_eq!(f.rt.dry_log[0]["address"], json!("system.notify"));
    assert_eq!(f.rt.timeline.len(), 2);
}

#[test]
fn a_surface_loads_through_resources_and_never_waits() {
    let mut f = Fixture::new("render");
    f.pack("gh", "", &pack(r#"abilities = { prs = echo.ability {
        title = "My Pull Requests", kind = "read", output = t.list(t.record("pr", { title = t.text(), url = t.url(), failing = t.bool() })),
        run = function(_, cx) task.wait(0.01) return { { title = "Fix CI", url = "https://x.dev/1", failing = true }, { title = "Docs", url = "https://x.dev/2", failing = false } } end,
        faces = { panel = function(view, cx)
            return ui.List { items = cx:load("gh.prs", {}), title = "title", subtitle = "url",
                intent = function(p) return p.failing and "urgent" or nil end, empty = "No open pull requests",
                actions = function(p) return { { title = "Nudge", run = function(item, cx) cx:toast("Nudged " .. item.title) end } } end }
        end },
    } }"#));
    let first = f.rt.render("gh", "ability:prs:panel", &json!({}), &json!({}), &Value::Null).unwrap();
    assert_eq!((first["_ui"].as_str(), first["state"].as_str(), first["empty"].as_str()), (Some("List"), Some("pending"), Some("No open pull requests")));
    f.rt.settle(Duration::from_secs(5));
    let second = f.rt.render("gh", "ability:prs:panel", &json!({}), &json!({}), &Value::Null).unwrap();
    assert_eq!(second["state"], json!("ok"));
    let rows = second["rows"].as_array().unwrap();
    assert_eq!((rows.len(), rows[0]["title"].as_str(), rows[0]["intent"].as_str(), rows[1].get("intent")), (2, Some("Fix CI"), Some("urgent"), None));
    assert_eq!(second["type"]["of"]["name"], json!("pr"));
    // An action's function is a handle the host can call back.
    let handler = rows[0]["actions"][0]["run"]["_handler"].as_u64().unwrap();
    let token = f.rt.handle("gh", "ability:prs:panel", handler, rows[0]["value"].clone(), false);
    f.rt.wait(token, Duration::from_secs(5)).unwrap();
    assert!(f.rt.events.contains(&Event::Toast { text: "Nudged Fix CI".into(), intent: String::new() }));
}

#[test]
fn a_broken_version_leaves_the_last_good_one_running() {
    let mut f = Fixture::new("reload");
    f.pack("v", "", &pack(r#"abilities = { n = echo.ability { kind = "read", run = function() return 1 end } }"#));
    assert_eq!(f.call("v.n", json!({})).unwrap(), json!(1));
    f.pack("v", "", "return echo.pack {");
    assert_eq!(f.state("v"), State::Broken);
    assert_eq!(f.rt.packs["v"].error.as_ref().unwrap().code, "E0200");
    assert_eq!(f.call("v.n", json!({})).unwrap(), json!(1));
    // A declaration mistake is caught the same way, before it replaces anything.
    f.pack("v", "", &pack(r#"abilities = { n = echo.ability { kind = "sometimes", run = function() return 2 end } }"#));
    assert_eq!((f.state("v"), f.call("v.n", json!({})).unwrap()), (State::Broken, json!(1)));
    f.pack("v", "", &pack(r#"abilities = { n = echo.ability { kind = "read", run = function() return 3 end } }"#));
    assert_eq!((f.state("v"), f.call("v.n", json!({})).unwrap()), (State::Ready, json!(3)));
    // No entry yet: an agent is still writing it.
    f.write("draft", &[("pack.toml", "id = \"draft\"\napi = 1\n")]).load("draft");
    assert_eq!(f.state("draft"), State::Drafting);
}

#[test]
fn a_runaway_pack_is_stopped_and_carries_on() {
    let mut f = Fixture::new("watchdog");
    f.pack("w", "", &pack(r#"abilities = {
        spin = echo.ability { kind = "read", run = function() while true do end end },
        ok = echo.ability { kind = "read", run = function() return "fine" end },
        hog = echo.ability { kind = "read", faces = { panel = function() local n = 0 while true do n += 1 end end }, run = function() return 1 end },
    }"#));
    let e = f.call("w.spin", json!({})).unwrap_err();
    assert_eq!(e.code, "E0204", "{e}");
    assert_eq!(f.call("w.ok", json!({})).unwrap(), json!("fine"));
    let started = std::time::Instant::now();
    let e = f.rt.render("w", "ability:hog:panel", &json!({}), &json!({}), &Value::Null).unwrap_err();
    assert_eq!(e.code, "E0204", "{e}");
    assert!(started.elapsed() < Duration::from_millis(200), "a render is stopped at 50 ms, not {:?}", started.elapsed());
}

#[test]
fn ai_gets_reads_and_only_marked_effects() {
    let mut f = Fixture::new("ai-tools");
    f.pack("m", "", &pack(r#"abilities = {
        look = echo.ability { kind = "read", run = function() return 1 end },
        act = echo.ability { kind = "effect", run = function() return 2 end },
        allowed = echo.ability { kind = "effect", ai = true, run = function() return 3 end },
    }"#));
    let mut ask = |address: &str| {
        let token = f.rt.start(Origin::Mcp, address, json!({}), Responder::Keep, false);
        f.rt.wait(token, Duration::from_secs(5))
    };
    assert_eq!(ask("m.look").unwrap(), json!(1));
    assert_eq!(ask("m.act").unwrap_err().code, "E0400");
    assert_eq!(ask("m.allowed").unwrap(), json!(3));
}

pub fn exists(path: &Path) -> bool {
    path.exists()
}

#[test]
fn an_effect_with_no_undo_calls_only_what_can_be_taken_back() {
    let mut f = Fixture::new("undo-rule");
    f.pack("u", "", &pack(r#"abilities = {
        lasting = echo.ability { kind = "effect", run = function() return 1 end },
        reversible = echo.ability { kind = "effect", run = function() return 2 end, undo = function() end },
        careless = echo.ability { kind = "effect", run = function(_, cx) return cx:call("u.lasting", {}) end },
        careful = echo.ability { kind = "effect", run = function(_, cx) cx:call("system.notify", { title = "hi" }) return cx:call("u.reversible", {}) end },
        owned = echo.ability { kind = "effect", run = function(_, cx) return cx:call("u.lasting", {}) end, undo = function() end },
    }"#));
    let e = f.call("u.careless", json!({})).unwrap_err();
    assert!(e.code == "E0311" && e.message.contains("u.lasting"), "{e}");
    // A notification leaves nothing to take back; an effect with an undo can be taken back.
    assert_eq!(f.call("u.careful", json!({})).unwrap(), json!(2));
    // With an undo of its own, it answers for what it calls.
    assert_eq!(f.call("u.owned", json!({})).unwrap(), json!(1));
}

#[test]
fn a_signature_change_that_breaks_a_caller_marks_it_broken() {
    let mut f = Fixture::new("signature");
    let callee = |input: &str| pack(&format!("abilities = {{ greet = echo.ability {{ kind = \"read\", input = {{ {input} }}, run = function(i) return \"hi\" end }} }}"));
    f.pack("callee", "", &callee("name = t.text():optional()"));
    f.pack("caller", "calls = [\"callee.greet\"]", &pack(r#"abilities = { go = echo.ability { kind = "read", run = function(_, cx) return cx:call("callee.greet", {}) end } }"#));
    f.rt.grant_all("caller");
    f.rt.check_dependencies();
    assert_eq!(f.state("caller"), State::Ready);
    // A new optional field breaks no one: a warning at most.
    f.write("callee", &[("init.luau", &callee("name = t.text():optional(), loud = t.bool():optional()"))]).load("callee");
    f.rt.check_dependencies();
    assert_eq!(f.state("caller"), State::Ready);
    // A field that must now be given breaks a caller written without it.
    f.write("callee", &[("init.luau", &callee("name = t.text(), loud = t.bool():optional()"))]).load("callee");
    f.rt.check_dependencies();
    assert_eq!(f.state("caller"), State::Broken);
    let e = f.rt.packs["caller"].error.clone().unwrap();
    assert!(e.code == "E0303" && e.message.contains("`name` can no longer be left out") && e.at.starts_with("init.luau:"), "{e:?}");
}

#[test]
fn a_tile_reads_what_an_earlier_tile_loaded() {
    let mut f = Fixture::new("tile-inputs");
    f.pack("day", "", &pack(r#"abilities = {
        events = echo.ability { kind = "read", output = t.list(t.text()), run = function() return { "Standup", "Lunch" } end },
        count = echo.ability { kind = "read", input = { items = t.list(t.text()) }, output = t.number(), run = function(i) return #i.items end },
        say = echo.ability { kind = "read", input = { text = t.text() }, output = t.text(), run = function(i) return i.text end },
    },
    boards = { morning = echo.board { title = "Morning", tiles = {
        { "day.events", size = "2x1" },
        { "day.count", inputs = { items = 1 } },
        { "day.say", inputs = { text = 1 } },
    } } }"#));
    assert_eq!(f.state("day"), State::Ready, "{:?}", f.rt.packs["day"].error);
    let draw = |f: &mut Fixture| super::surface::board(&mut f.rt, "day", "morning", &json!({})).unwrap();
    // The first draw starts the loads; a tile that reads another waits until it has.
    let first = draw(&mut f);
    assert!(matches!(first["tiles"][1]["content"]["_ui"].as_str(), Some("Loading" | "Stat")), "{first:#}");
    f.rt.settle(Duration::from_secs(5));
    let _ = draw(&mut f);
    f.rt.settle(Duration::from_secs(5));
    let board = draw(&mut f);
    assert_eq!(board["tiles"][1]["args"]["items"], json!(["Standup", "Lunch"]), "{board:#}");
    // A text input takes the list as it reads.
    assert!(board["tiles"][2]["args"]["text"].as_str().is_some_and(|t| t.contains("Standup")), "{board:#}");
    // A tile can only read what is above it.
    f.write("bad", &[("pack.toml", "id = \"bad\"\napi = 1\n"), ("init.luau", &pack(r#"abilities = { a = echo.ability { kind = "read", run = function() return 1 end } },
        boards = { b = echo.board { tiles = { { "bad.a", inputs = { x = 1 } } } } }"#))]).load("bad");
    assert_eq!(f.rt.packs["bad"].error.as_ref().map(|e| e.code), Some("E0202"));
}

#[test]
fn a_pack_describes_and_cannot_reach_raw_rune_ui() {
    let mut f = Fixture::new("no-raw-ui");
    f.pack("raw", "", "local ui = require(\"@rune/ui\")\nlocal echo = require(\"@echo\")\nreturn echo.pack { abilities = { a = echo.ability { kind = \"read\", run = function() return 1 end } } }");
    let e = f.rt.packs["raw"].error.clone().expect("raw @rune/ui is refused");
    assert!(e.message.contains("@rune/ui"), "{e:?}");
}

#[test]
fn a_suggested_hotkey_must_be_a_chord() {
    let mut f = Fixture::new("hotkey");
    f.pack("h", "", &pack(r#"abilities = { a = echo.ability { kind = "read", hotkey = "super+alt+p", run = function() return 1 end } }"#));
    assert_eq!(f.rt.packs["h"].index.abilities["a"].hotkey, "super+alt+p");
    f.write("h", &[("init.luau", &pack(r#"abilities = { a = echo.ability { kind = "read", hotkey = "banana", run = function() return 1 end } }"#))]).load("h");
    assert_eq!(f.rt.packs["h"].error.as_ref().map(|e| e.code), Some("E0202"));
}

#[test]
fn an_inbox_report_draws_again_through_the_code_as_it_is_now() {
    let mut f = Fixture::new("inbox-replay");
    let panel = |label: &str| pack(&format!(r#"abilities = {{ list = echo.ability {{ kind = "read", output = t.list(t.text()), run = function() return {{ "a" }} end,
        faces = {{ panel = function(view, cx) return ui.List {{ items = cx:load("r.list", {{}}), heading = "{label}", empty = "None" }} end }} }} }}"#));
    f.pack("r", "", &panel("Before"));
    let mut report = super::inbox::Report { pack: "r".into(), target: "ability:list:panel".into(), note: "say After".into(), ..Default::default() };
    report.loads.insert("r.list".into(), json!(["x", "y"]));
    // The agent changes the code; the person's state is drawn again through it.
    f.write("r", &[("init.luau", &panel("After"))]).load("r");
    f.rt.set_scene(Some(super::inbox::scene(&report, "test")));
    let blocks = f.rt.render("r", "ability:list:panel", &json!({}), &json!({}), &Value::Null).unwrap();
    f.rt.set_scene(None);
    let tree = super::blocks::tree(&blocks);
    assert_eq!(blocks["heading"], json!("After"), "{blocks:#}");
    assert!(tree.contains("x") && tree.contains("y"), "{tree}");
}

#[test]
fn a_living_icon_says_what_to_draw() {
    let mut f = Fixture::new("living");
    f.pack("inbox", "", &pack(r#"abilities = { unread = echo.ability { kind = "read", output = t.number(), run = function() return 3 end,
        faces = { icon = function(out) return { badge = out, progress = 0.5, pulse = out > 0 } end } } }"#));
    let mut host = super::host::Host::new();
    host.rt = std::mem::replace(&mut f.rt, super::runtime::Runtime::ephemeral());
    host.refresh_living();
    host.rt.settle(Duration::from_secs(5));
    assert!(host.refresh_living(), "the icon changed once its output loaded");
    let living = host.living["inbox.unread"].clone();
    assert_eq!((living.badge.as_deref(), living.progress, living.pulse), (Some("3"), Some(0.5), true));
}
