//! Stores: live, typed, versioned data. A surface that reads one is drawn again when it changes;
//! `keep` retains past versions, so a board can show yesterday.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::index::StoreDecl;

/// Stores the host itself keeps current, and the ability whose grant reading one needs.
pub const BUILTIN: &[(&str, &str)] = &[
    ("clipboard", "clipboard.history"), ("windows", "windows.list"), ("calendar", "calendar.next"), ("notes", "notes.list"),
    ("battery", ""), ("network", ""), ("now", ""),
];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Saved {
    history: Vec<(f64, Value)>,
}

#[derive(Clone, Debug, Default)]
pub struct Store {
    pub decl: StoreDecl,
    /// Oldest first; the last is the value now.
    pub history: Vec<(f64, Value)>,
    /// Seconds of history to retain; zero keeps only the current value.
    keep: f64,
}

#[derive(Default)]
pub struct Stores {
    /// By `<pack>.<name>`.
    map: BTreeMap<String, Store>,
    /// Moves with every write: what surfaces compare to know they are stale.
    pub version: u64,
}

fn file(key: &str) -> PathBuf {
    let dir = super::state_dir().join("stores");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{key}.json"))
}

impl Stores {
    /// Registers a pack's store, reading what it persisted. A store already live keeps its value
    /// across a reload.
    pub fn declare(&mut self, pack: &str, name: &str, decl: &StoreDecl) {
        let key = format!("{pack}.{name}");
        let keep = super::triggers::parse_every(&decl.keep).unwrap_or(0.0);
        if let Some(store) = self.map.get_mut(&key) {
            store.decl = decl.clone();
            store.keep = keep;
            return;
        }
        let mut history = Vec::new();
        if decl.persist
            && let Some(saved) = std::fs::read_to_string(file(&key)).ok().and_then(|t| serde_json::from_str::<Saved>(&t).ok()) {
            history = saved.history;
        }
        self.map.insert(key, Store { decl: decl.clone(), history, keep });
    }

    /// Forgets the stores a pack no longer declares.
    pub fn retain(&mut self, pack: &str, names: &[&String]) {
        let prefix = format!("{pack}.");
        self.map.retain(|key, _| key.strip_prefix(&prefix).is_none_or(|name| names.iter().any(|n| n.as_str() == name)));
    }

    pub fn exists(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    pub fn names(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }

    /// The value now, or as it was at `at`: the last version written at or before it.
    pub fn get(&self, key: &str, at: Option<f64>) -> Value {
        let Some(store) = self.map.get(key) else { return Value::Null };
        let found = match at {
            Some(t) => store.history.iter().rev().find(|(when, _)| *when <= t),
            None => store.history.last(),
        };
        found.map(|(_, v)| v.clone()).unwrap_or_else(|| if at.is_none() { store.decl.initial.clone() } else { Value::Null })
    }

    /// Writes a new version. The value is fitted to the store's type first.
    pub fn put(&mut self, key: &str, value: Value, now: f64) -> Result<(), String> {
        let Some(store) = self.map.get_mut(key) else { return Err(format!("no store {key}")) };
        let value = match &store.decl.ty {
            Some(ty) => ty.coerce(value)?,
            None => value,
        };
        if store.history.last().is_some_and(|(_, last)| *last == value) {
            return Ok(());
        }
        if store.keep > 0.0 {
            store.history.retain(|(when, _)| now - *when <= store.keep);
            // Even a store written every second stays bounded.
            if store.history.len() >= 5000 {
                store.history.drain(..1000);
            }
        } else {
            store.history.clear();
        }
        store.history.push((now, value));
        if store.decl.persist
            && let Ok(text) = serde_json::to_string(&Saved { history: store.history.clone() }) {
            let _ = std::fs::write(file(key), text);
        }
        self.version += 1;
        Ok(())
    }

    /// When each kept version was written.
    pub fn times(&self, key: &str) -> Vec<f64> {
        self.map.get(key).map(|s| s.history.iter().map(|(t, _)| *t).collect()).unwrap_or_default()
    }
}

/// A built-in store's value, read from the feature that owns it.
pub fn builtin(name: &str) -> Value {
    match name {
        "now" => {
            let now = chrono::Local::now();
            json!({ "time": now.timestamp(), "iso": now.to_rfc3339(), "date": now.format("%Y-%m-%d").to_string(), "clock": now.format("%H:%M").to_string(), "weekday": now.format("%A").to_string() })
        }
        "battery" => battery(),
        "network" => network(),
        _ => builtin_ability(name),
    }
}

/// A built-in store that mirrors a built-in ability: the clipboard's history, the open windows.
fn builtin_ability(name: &str) -> Value {
    let Some((_, address)) = BUILTIN.iter().find(|(n, a)| *n == name && !a.is_empty()) else { return Value::Null };
    super::builtins::call_now(address, json!({}))
}

/// `{ percent, charging, present }`, from the first battery the kernel lists.
pub fn battery() -> Value {
    let found = std::fs::read_dir("/sys/class/power_supply").into_iter().flatten().flatten().map(|e| e.path())
        .find(|p| std::fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Battery"));
    let Some(dir) = found else { return json!({ "present": false, "percent": 100, "charging": true }) };
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default().trim().to_string();
    json!({ "present": true, "percent": read("capacity").parse::<i64>().unwrap_or(100), "charging": matches!(read("status").as_str(), "Charging" | "Full" | "Not charging") })
}

/// `{ online, interface }`: whether any interface but loopback is up.
pub fn network() -> Value {
    let up = std::fs::read_dir("/sys/class/net").into_iter().flatten().flatten()
        .filter(|e| e.file_name() != "lo")
        .find(|e| std::fs::read_to_string(e.path().join("operstate")).is_ok_and(|s| s.trim() == "up"))
        .map(|e| e.file_name().to_string_lossy().to_string());
    json!({ "online": up.is_some(), "interface": up.unwrap_or_default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::types::Type;

    #[test]
    fn a_store_keeps_its_past() {
        let mut stores = Stores::default();
        stores.declare("ci", "builds", &StoreDecl { ty: Some(Type::of("number")), keep: "1h".into(), persist: false, initial: json!(0) });
        assert_eq!(stores.get("ci.builds", None), json!(0));
        stores.put("ci.builds", json!(1), 100.0).unwrap();
        stores.put("ci.builds", json!("2"), 200.0).unwrap();
        stores.put("ci.builds", json!(2), 250.0).unwrap();
        assert_eq!(stores.version, 2);
        assert_eq!((stores.get("ci.builds", None), stores.get("ci.builds", Some(150.0)), stores.get("ci.builds", Some(50.0))), (json!(2), json!(1), Value::Null));
        assert!(stores.put("ci.builds", json!([1]), 300.0).is_err());
        stores.put("ci.builds", json!(3), 4000.0).unwrap();
        assert_eq!(stores.times("ci.builds"), vec![4000.0]);
    }
}
