//! The Rust tier end to end: a real pack built with cargo for wasm32-wasip2, loaded, and called
//! through the broker. Skipped, with a note, where the target isn't installed.

use std::time::Duration;

use serde_json::json;

use crate::packs::runtime::{Runtime, State};
use crate::packs::tests::Fixture;

const LIB: &str = r#"
mod calls;
mod echo;
use echo::{Ability, Cx, Pack, Result, Store, Verb, t};
use serde_json::{Value, json};

pub fn pack() -> Pack {
    Pack::new("Rusty")
        .store("count", Store::new(t::number()).initial(json!(0)))
        .ability("add", Ability::read("Add").input([("a", t::number()), ("b", t::number())]).output(t::number())
            .run(|i, _| Ok(json!(i["a"].as_f64().unwrap_or(0.0) + i["b"].as_f64().unwrap_or(0.0) + __EXTRA__))))
        .ability("convert", Ability::read("Convert").output(t::any())
            .run(|_, cx| calls::calc_convert(cx, 2.0, "km", "m")))
        .ability("escape", Ability::read("Escape").output(t::any()).run(escape))
        .ability("notify", Ability::read("Notify").run(|_, cx| cx.call("system.notify", json!({ "title": "hi" }))))
        .ability("peek", Ability::read("Peek").run(|_, cx| cx.call("clipboard.read", json!({}))))
        .ability("spin", Ability::read("Spin").run(|_, _| { let mut n = 0u64; loop { n = std::hint::black_box(n.wrapping_add(1)); } }))
        .ability("boom", Ability::read("Boom").run(|_, _| { println!("about to fail"); panic!("kaboom") }))
        .ability("bump", Ability::effect("Bump").output(t::number()).run(bump).undo(|_, out, cx| {
            cx.put("count", json!(out.as_f64().unwrap_or(1.0) - 1.0))?;
            Ok(Value::Null)
        }))
        .verb("shout", Verb::new("text", "Shout").kind("read").run(|v, _| Ok(json!(v.as_str().unwrap_or("").to_uppercase()))))
}

fn bump(_: Value, cx: &Cx) -> Result {
    let now = cx.store("count")?.as_f64().unwrap_or(0.0) + 1.0;
    cx.put("count", json!(now))?;
    Ok(json!(now))
}

/// Everything a component could reach for without the broker; all of it must fail.
fn escape(_: Value, _: &Cx) -> Result {
    Ok(json!({
        "file": std::fs::read_to_string("/etc/hostname").is_ok(),
        "dir": std::fs::read_dir("/").is_ok(),
        "net": std::net::TcpStream::connect("1.1.1.1:443").is_ok(),
        "env": std::env::vars().count(),
        "args": std::env::args().count(),
    }))
}
"#;

const CARGO: &str = "[package]\nname = \"__CRATE__\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nwit-bindgen = \"0.57\"\nserde_json = \"1\"\n\n[profile.release]\nopt-level = 1\n";

fn ready() -> bool {
    match super::toolchain() {
        Ok(()) => true,
        Err(how) => {
            eprintln!("skipped: Rust packs need the {} target ({how})", super::TARGET);
            false
        }
    }
}

/// Each test has its own crate name: they build at once into one shared target folder.
fn write(f: &mut Fixture, krate: &str, extra: &str) {
    f.write("rusty", &[
        ("pack.toml", "id = \"rusty\"\napi = 1\n"),
        ("Cargo.toml", &CARGO.replace("__CRATE__", krate)),
        ("src/lib.rs", &LIB.replace("__EXTRA__", extra)),
    ]);
}

#[test]
fn a_rust_pack_runs_through_the_broker_and_nowhere_else() {
    if !ready() {
        return;
    }
    let mut f = Fixture::new("wasm");
    write(&mut f, "rusty-broker", "0.0");
    f.load("rusty");
    let pack = &f.rt.packs["rusty"];
    assert_eq!(pack.state, State::Ready, "{:?}", pack.error);
    assert!(pack.wasm.is_some() && pack.index.abilities.contains_key("add") && pack.index.verbs.contains_key("shout"));
    assert!(f.dir("rusty").join("src/calls.rs").is_file() && f.dir("rusty").join("wit/echospot.wit").is_file());

    // Inputs and outputs are checked like any pack's.
    assert_eq!(f.call("rusty.add", json!({ "a": 2, "b": 3 })).unwrap(), json!(5.0));
    assert_eq!(f.call("rusty.add", json!({ "a": "two" })).unwrap_err().code, "E0301");
    // A typed call from calls.rs reaches a built-in.
    assert!(f.call("rusty.convert", json!({})).is_ok());

    // No file, folder, network, environment or arguments: the broker is the only way out.
    let escaped = f.call("rusty.escape", json!({})).unwrap();
    assert_eq!(escaped, json!({ "file": false, "dir": false, "net": false, "env": 0, "args": 0 }), "{escaped}");

    // A read stays a read, and a pack can't call what it wasn't granted.
    assert_eq!(f.call("rusty.notify", json!({})).unwrap_err().code, "E0310");
    assert!(matches!(f.call("rusty.peek", json!({})).unwrap_err().code, "E0400" | "E0401"));

    // A panic stops the call and its message is in the log; so is what it printed.
    assert_eq!(f.call("rusty.boom", json!({})).unwrap_err().code, "E0208");
    let logs: Vec<String> = f.rt.shared.logs.borrow().iter().map(|l| l.text.clone()).collect();
    assert!(logs.iter().any(|l| l.contains("kaboom")) && logs.iter().any(|l| l == "about to fail"), "{logs:#?}");

    // A runaway loop is stopped when its compute runs out.
    super::FUEL_NOW.store(50_000_000, std::sync::atomic::Ordering::Relaxed);
    let spun = f.call("rusty.spin", json!({}));
    super::FUEL_NOW.store(super::FUEL, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(spun.unwrap_err().code, "E0209");

    // Stores, an effect, and taking it back with the pack's own undo.
    assert_eq!(f.call("rusty.bump", json!({})).unwrap(), json!(1.0));
    assert_eq!(f.call("rusty.bump", json!({})).unwrap(), json!(2.0));
    assert_eq!(f.rt.shared.stores.borrow().get("rusty.count", None), json!(2.0));
    let id = f.rt.timeline.back().expect("an effect").id;
    for token in f.rt.undo(id).unwrap() {
        f.rt.wait(token, Duration::from_secs(30)).unwrap();
    }
    assert_eq!(f.rt.shared.stores.borrow().get("rusty.count", None), json!(1.0));

    // Verbs run in the component too.
    let token = f.rt.verb("rusty", "shout", json!("hey"), false);
    assert_eq!(f.rt.wait(token, Duration::from_secs(30)).unwrap(), json!("HEY"));
}

#[test]
fn the_app_builds_on_a_thread_and_the_last_good_version_keeps_running() {
    if !ready() {
        return;
    }
    let mut f = Fixture::new("wasm-async");
    write(&mut f, "rusty-async", "0.0");
    f.load("rusty");
    assert_eq!(f.call("rusty.add", json!({ "a": 1, "b": 1 })).unwrap(), json!(2.0));

    // As the app does: no building inside `load`.
    let mut rt = std::mem::replace(&mut f.rt, Runtime::ephemeral());
    rt.build_inline = None;
    write(&mut f, "rusty-async", "100.0");
    rt.load(&f.dir("rusty"), false);
    assert!(rt.packs["rusty"].building.is_some(), "a build started");
    // The old version still answers while the new one builds.
    assert_eq!(rt.call("rusty.add", json!({ "a": 1, "b": 1 }), false).unwrap(), json!(2.0));
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    while rt.packs["rusty"].building.is_some() && std::time::Instant::now() < deadline {
        rt.pump(&mut rune::Tx::default());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(rt.packs["rusty"].state, State::Ready, "{:?}", rt.packs["rusty"].error);
    assert_eq!(rt.call("rusty.add", json!({ "a": 1, "b": 1 }), false).unwrap(), json!(102.0));

    // A version that doesn't compile leaves the last good one running, and says where.
    f.write("rusty", &[("src/lib.rs", "mod calls;\nmod echo;\npub fn pack() -> echo::Pack { nope }\n")]);
    rt.load(&f.dir("rusty"), false);
    while rt.packs["rusty"].building.is_some() && std::time::Instant::now() < deadline {
        rt.pump(&mut rune::Tx::default());
        std::thread::sleep(Duration::from_millis(20));
    }
    let pack = &rt.packs["rusty"];
    assert_eq!(pack.state, State::Broken);
    let error = pack.error.clone().unwrap();
    assert!(error.code == "E0206" && error.at.starts_with("src/lib.rs:3"), "{error:?}");
    assert_eq!(rt.call("rusty.add", json!({ "a": 1, "b": 1 }), false).unwrap(), json!(102.0));
}
