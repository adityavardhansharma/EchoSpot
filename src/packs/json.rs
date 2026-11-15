//! Values across the VM boundary. The host thinks in JSON; a VM takes `Data` or Luau values.

use rune_luau::Data;
use rune_luau::mlua::{self, Lua, Value as Lv};
use serde_json::{Map, Number, Value};

const MAX_DEPTH: usize = 64;

pub fn to_data(v: &Value) -> Data {
    match v {
        Value::Null => Data::Nil,
        Value::Bool(b) => Data::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Data::Integer(i),
            None => Data::Number(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => Data::String(s.clone()),
        Value::Array(a) => Data::List(a.iter().map(to_data).collect()),
        Value::Object(m) => Data::Map(m.iter().map(|(k, v)| (k.clone(), to_data(v))).collect()),
    }
}

pub fn from_data(d: &Data) -> Value {
    match d {
        Data::Nil => Value::Null,
        Data::Bool(b) => Value::Bool(*b),
        Data::Integer(i) => Value::Number((*i).into()),
        Data::Number(n) => Number::from_f64(*n).map(Value::Number).unwrap_or(Value::Null),
        Data::String(s) => Value::String(s.clone()),
        Data::List(l) => Value::Array(l.iter().map(from_data).collect()),
        Data::Map(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), from_data(v))).collect()),
    }
}

/// A Luau value as JSON. A table with only `1..=n` keys is an array (an empty one too); any
/// other is an object, its number keys written as text. Functions and userdata are dropped.
pub fn from_lua(v: &Lv) -> Value {
    fn walk(v: &Lv, depth: usize) -> Value {
        match v {
            Lv::Boolean(b) => Value::Bool(*b),
            Lv::Integer(i) => Value::Number((*i as i64).into()),
            Lv::Number(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => Value::Number((*n as i64).into()),
            Lv::Number(n) => Number::from_f64(*n).map(Value::Number).unwrap_or(Value::Null),
            Lv::String(s) => Value::String(s.to_string_lossy()),
            Lv::Table(t) if depth < MAX_DEPTH => {
                let len = t.raw_len();
                let mut map = Map::new();
                let mut list = Vec::with_capacity(len);
                let mut array = true;
                for pair in t.pairs::<Lv, Lv>() {
                    let Ok((k, x)) = pair else { continue };
                    let x = walk(&x, depth + 1);
                    match &k {
                        Lv::Integer(i) if array && *i as usize == list.len() + 1 => list.push(x),
                        Lv::Number(n) if array && *n == (list.len() + 1) as f64 => list.push(x),
                        _ => {
                            if array {
                                array = false;
                                for (i, item) in list.drain(..).enumerate() {
                                    map.insert((i + 1).to_string(), item);
                                }
                            }
                            let key = match &k {
                                Lv::String(s) => s.to_string_lossy(),
                                Lv::Integer(i) => i.to_string(),
                                Lv::Number(n) => n.to_string(),
                                _ => continue,
                            };
                            if !x.is_null() {
                                map.insert(key, x);
                            }
                        }
                    }
                }
                if array { Value::Array(list) } else { Value::Object(map) }
            }
            _ => Value::Null,
        }
    }
    walk(v, 0)
}

pub fn to_lua(lua: &Lua, v: &Value) -> mlua::Result<Lv> {
    Ok(match v {
        Value::Null => Lv::Nil,
        Value::Bool(b) => Lv::Boolean(*b),
        Value::Number(n) => Lv::Number(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => Lv::String(lua.create_string(s)?),
        Value::Array(a) => {
            let t = lua.create_table_with_capacity(a.len(), 0)?;
            for (i, x) in a.iter().enumerate() {
                t.raw_set(i + 1, to_lua(lua, x)?)?;
            }
            Lv::Table(t)
        }
        Value::Object(m) => {
            let t = lua.create_table_with_capacity(0, m.len())?;
            for (k, x) in m {
                t.raw_set(k.as_str(), to_lua(lua, x)?)?;
            }
            Lv::Table(t)
        }
    })
}

/// A stable text for a value: object keys sorted (serde_json's map is), so it can key a cache.
pub fn canonical(v: &Value) -> String {
    // An empty Luau table is `[]`; as an input it means "no arguments", the same as `{}`.
    match v {
        Value::Array(a) if a.is_empty() => "{}".into(),
        _ => v.to_string(),
    }
}

/// A short, single-line summary of a value for the audit log and a toast.
pub fn summary(v: &Value, max: usize) -> String {
    let text = match v {
        Value::String(s) => s.replace('\n', " "),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    if text.chars().count() > max { format!("{}…", text.chars().take(max.saturating_sub(1)).collect::<String>()) } else { text }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn luau_tables_become_arrays_or_objects() {
        let lua = Lua::new();
        let v: Lv = lua.load(r#"return { list = { 1, 2, "x" }, empty = {}, mixed = { "a", size = "2x1" }, n = 1.5, f = print }"#).eval().unwrap();
        assert_eq!(from_lua(&v), json!({ "list": [1, 2, "x"], "empty": [], "mixed": { "1": "a", "size": "2x1" }, "n": 1.5 }));
        let back = to_lua(&lua, &json!({ "a": [1, { "b": true }], "c": null })).unwrap();
        assert_eq!(from_lua(&back), json!({ "a": [1, { "b": true }] }));
        assert_eq!(from_data(&to_data(&json!({ "a": [1, 2.5, "x", null] }))), json!({ "a": [1, 2.5, "x", null] }));
    }
}
