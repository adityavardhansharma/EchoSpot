//! Types: what a value is. A schema is real data, declared once in Luau with `@echo/types`, and
//! the host derives everything else from it: validation, forms, CLI arguments, JSON Schema for
//! AI tools, Luau type definitions, default faces, and which verbs a value offers.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// Every core type name, with the shape its values take.
pub const CORE: &[(&str, &str)] = &[
    ("text", "string"), ("markdown", "string"), ("code", "string"), ("number", "number"), ("bool", "boolean"),
    ("quantity", "{ value: number, unit: string }"), ("money", "{ amount: number, currency: string }"),
    ("currency", "string"), ("percent", "number"), ("date", "string"), ("time", "string"), ("datetime", "string"),
    ("duration", "number"), ("url", "string"), ("file", "string"), ("folder", "string"), ("image", "string"),
    ("color", "string"), ("app", "string"), ("window", "{ address: string, title: string, class: string }"),
    ("event", "{ title: string, start: string, [\"end\"]: string?, location: string? }"),
    ("person", "{ name: string, email: string? }"), ("place", "{ name: string, lat: number?, lon: number? }"),
    ("table", "{ columns: { string }, rows: { { any } } }"), ("series", "{ { t: number, v: number } }"),
    ("list", "{ T }"), ("record", "{ … }"), ("enum", "string"), ("any", "any"),
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Type {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    /// A list's element type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub of: Option<Box<Type>>,
    /// A record's fields.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Type>,
    /// A record's name (`pr`), which verbs and Send to… match on.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// An enum's values.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    /// What a form shows beside the field.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

impl Type {
    pub fn of(kind: &str) -> Self {
        Self { kind: kind.into(), ..Default::default() }
    }
    pub fn list(of: Type) -> Self {
        Self { kind: "list".into(), of: Some(Box::new(of)), ..Default::default() }
    }
    pub fn record(fields: impl IntoIterator<Item = (&'static str, Type)>) -> Self {
        Self { kind: "record".into(), fields: fields.into_iter().map(|(k, t)| (k.to_string(), t)).collect(), ..Default::default() }
    }
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Reads a declaration as `@echo/types` built it. A bare table of fields is a record.
    pub fn from_json(v: &Value) -> Result<Self, String> {
        let Some(map) = v.as_object() else { return Err(format!("a type must be made with @echo/types, not {v}")) };
        let Some(kind) = map.get("type").and_then(Value::as_str) else {
            // `input = { amount = t.number() }`: the fields themselves.
            let mut fields = BTreeMap::new();
            for (k, f) in map {
                fields.insert(k.clone(), Type::from_json(f).map_err(|e| format!("{k}: {e}"))?);
            }
            return Ok(Type { kind: "record".into(), fields, ..Default::default() });
        };
        if !CORE.iter().any(|(k, _)| *k == kind) {
            let near = super::error::nearest(kind, CORE.iter().map(|(k, _)| *k));
            return Err(format!("no type {kind:?}{}", if near.is_empty() { String::new() } else { format!(" (did you mean {}?)", near.join(", ")) }));
        }
        let mut t = Type::of(kind);
        t.optional = map.get("optional").and_then(Value::as_bool).unwrap_or(false);
        t.name = map.get("name").and_then(Value::as_str).unwrap_or("").into();
        t.label = map.get("label").and_then(Value::as_str).unwrap_or("").into();
        t.default = map.get("default").filter(|d| !d.is_null()).cloned();
        if let Some(of) = map.get("of").filter(|o| !o.is_null()) {
            t.of = Some(Box::new(Type::from_json(of).map_err(|e| format!("list element: {e}"))?));
        }
        if let Some(fields) = map.get("fields").and_then(Value::as_object) {
            for (k, f) in fields {
                t.fields.insert(k.clone(), Type::from_json(f).map_err(|e| format!("{k}: {e}"))?);
            }
        }
        if let Some(values) = map.get("values").and_then(Value::as_array) {
            t.values = values.iter().filter_map(|s| s.as_str().map(String::from)).collect();
        }
        Ok(t)
    }

    /// How the type reads in a signature: `list<pr>`, `money`, `text?`.
    pub fn label(&self) -> String {
        let base = match self.kind.as_str() {
            "list" => format!("list<{}>", self.of.as_ref().map(|t| t.label()).unwrap_or_else(|| "any".into())),
            "record" if !self.name.is_empty() => self.name.clone(),
            "record" => format!("{{ {} }}", self.fields.iter().map(|(k, t)| format!("{k}: {}", t.label())).collect::<Vec<_>>().join(", ")),
            "enum" => self.values.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(" | "),
            k => k.to_string(),
        };
        if self.optional { format!("{base}?") } else { base }
    }

    /// The Luau type, for `echospot.d.luau`.
    pub fn luau(&self) -> String {
        let base = match self.kind.as_str() {
            "list" => format!("{{ {} }}", self.of.as_ref().map(|t| t.luau()).unwrap_or_else(|| "any".into())),
            "record" => format!("{{ {} }}", self.fields.iter().map(|(k, t)| format!("{k}: {}", t.luau())).collect::<Vec<_>>().join(", ")),
            "enum" if !self.values.is_empty() => self.values.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(" | "),
            k => CORE.iter().find(|(n, _)| *n == k).map(|(_, shape)| shape.to_string()).unwrap_or_else(|| "any".into()),
        };
        if self.optional { format!("({base})?") } else { base }
    }

    /// JSON Schema: what an AI tool definition and MCP carry.
    pub fn json_schema(&self) -> Value {
        let mut s = match self.kind.as_str() {
            "number" | "percent" | "duration" => json!({ "type": "number" }),
            "bool" => json!({ "type": "boolean" }),
            "quantity" => json!({ "type": "object", "properties": { "value": { "type": "number" }, "unit": { "type": "string" } }, "required": ["value", "unit"] }),
            "money" => json!({ "type": "object", "properties": { "amount": { "type": "number" }, "currency": { "type": "string" } }, "required": ["amount", "currency"] }),
            "list" | "series" => json!({ "type": "array", "items": self.of.as_ref().map(|t| t.json_schema()).unwrap_or_else(|| json!({})) }),
            "record" => {
                let props: Map<String, Value> = self.fields.iter().map(|(k, t)| (k.clone(), t.json_schema())).collect();
                let required: Vec<&String> = self.fields.iter().filter(|(_, t)| !t.optional && t.default.is_none()).map(|(k, _)| k).collect();
                json!({ "type": "object", "properties": props, "required": required })
            }
            "enum" => json!({ "type": "string", "enum": self.values }),
            "window" | "event" | "person" | "place" | "table" => json!({ "type": "object" }),
            "any" => json!({}),
            "url" => json!({ "type": "string", "format": "uri" }),
            "date" => json!({ "type": "string", "format": "date" }),
            "datetime" => json!({ "type": "string", "format": "date-time" }),
            _ => json!({ "type": "string" }),
        };
        if let (Some(map), false) = (s.as_object_mut(), self.kind == "any") {
            let about = if self.label.is_empty() { self.kind.clone() } else { format!("{} ({})", self.label, self.kind) };
            if !matches!(self.kind.as_str(), "text" | "number" | "bool" | "record" | "list") || !self.label.is_empty() {
                map.insert("description".into(), Value::String(about));
            }
        }
        s
    }

    /// Fits `v` to the type, filling defaults and turning what a command line or a form typed
    /// (`"50"`) into what the type holds (`50`). The error names the path that didn't fit.
    pub fn coerce(&self, v: Value) -> Result<Value, String> {
        if v.is_null() {
            return match (&self.default, self.optional) {
                (Some(d), _) => Ok(d.clone()),
                (None, true) => Ok(Value::Null),
                (None, false) if self.kind == "any" => Ok(Value::Null),
                // A record whose fields are all optional may be left out whole.
                (None, false) if self.kind == "record" => self.coerce(json!({})),
                (None, false) => Err(format!("expected {}, got nothing", self.label())),
            };
        }
        let wrong = |v: &Value| Err(format!("expected {}, got {}", self.label(), short(v)));
        match self.kind.as_str() {
            "any" => Ok(v),
            "number" | "percent" | "duration" => match &v {
                Value::Number(_) => Ok(v),
                Value::String(s) => s.trim().replace(',', "").parse::<f64>().ok().and_then(|n| serde_json::Number::from_f64(n)).map(Value::Number).ok_or(()).or_else(|_| wrong(&v)),
                _ => wrong(&v),
            },
            "bool" => match &v {
                Value::Bool(_) => Ok(v),
                Value::String(s) if matches!(s.as_str(), "true" | "yes" | "1") => Ok(Value::Bool(true)),
                Value::String(s) if matches!(s.as_str(), "false" | "no" | "0") => Ok(Value::Bool(false)),
                _ => wrong(&v),
            },
            "enum" => match v.as_str() {
                Some(s) if self.values.is_empty() || self.values.iter().any(|x| x == s) => Ok(v),
                _ => Err(format!("expected one of {}, got {}", self.values.join(", "), short(&v))),
            },
            "currency" => match v.as_str() {
                Some(s) if !s.trim().is_empty() => Ok(Value::String(s.trim().to_uppercase())),
                _ => wrong(&v),
            },
            "url" => match v.as_str() {
                Some(s) if s.contains("://") || s.starts_with("mailto:") => Ok(v),
                _ => wrong(&v),
            },
            "list" | "series" => {
                let Value::Array(items) = v else { return wrong(&v) };
                let Some(of) = &self.of else { return Ok(Value::Array(items)) };
                items.into_iter().enumerate().map(|(i, x)| of.coerce(x).map_err(|e| format!("[{}]: {e}", i + 1))).collect::<Result<Vec<_>, _>>().map(Value::Array)
            }
            "record" => {
                // An empty Luau table arrives as an empty array.
                let mut map = match v {
                    Value::Object(m) => m,
                    Value::Array(a) if a.is_empty() => Map::new(),
                    other => return wrong(&other),
                };
                for (k, t) in &self.fields {
                    let fitted = t.coerce(map.remove(k).unwrap_or(Value::Null)).map_err(|e| format!("{k}: {e}"))?;
                    if !fitted.is_null() {
                        map.insert(k.clone(), fitted);
                    }
                }
                Ok(Value::Object(map))
            }
            "money" => match &v {
                Value::Object(m) if m.get("amount").is_some_and(Value::is_number) && m.get("currency").is_some_and(Value::is_string) => Ok(v),
                _ => wrong(&v),
            },
            "quantity" => match &v {
                Value::Object(m) if m.get("value").is_some_and(Value::is_number) => Ok(v),
                _ => wrong(&v),
            },
            "window" | "event" | "person" | "place" | "table" => if v.is_object() { Ok(v) } else { wrong(&v) },
            // A date or a time may be a number of seconds as well as text.
            "date" | "time" | "datetime" => if v.is_string() || v.is_number() { Ok(v) } else { wrong(&v) },
            _ => match v {
                Value::String(_) => Ok(v),
                Value::Number(n) => Ok(Value::String(n.to_string())),
                other => wrong(&other),
            },
        }
    }

    /// Whether a value of type `other` can be sent to an input of this type.
    pub fn accepts(&self, other: &Type) -> bool {
        if self.kind == "any" || self.kind == other.kind && (self.kind != "record" || self.name.is_empty() || self.name == other.name) {
            return self.kind != "list" || match (&self.of, &other.of) {
                (Some(a), Some(b)) => a.accepts(b),
                _ => true,
            };
        }
        // Anything written down can stand in for text; a file is a path, a url is a line.
        self.kind == "text" && matches!(other.kind.as_str(), "markdown" | "code" | "url" | "file" | "folder" | "color" | "currency" | "date" | "time" | "datetime" | "app" | "enum")
            || self.kind == "number" && matches!(other.kind.as_str(), "percent" | "duration")
            || self.kind == "file" && matches!(other.kind.as_str(), "image" | "folder")
    }

    /// The one input field a value of `other` would fill, when this is an ability's input.
    pub fn slot_for(&self, other: &Type) -> Option<&str> {
        let mut fits = self.fields.iter().filter(|(_, t)| t.accepts(other));
        let first = fits.next()?;
        // Exactly one field takes it, and the rest can be left out.
        let rest_optional = self.fields.iter().all(|(k, t)| k == first.0 || t.optional || t.default.is_some());
        (fits.next().is_none() && rest_optional || self.fields.len() == 1).then_some(first.0.as_str())
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 60 { format!("{}…", s.chars().take(60).collect::<String>()) } else { s }
}

/// The type a bare value looks like, where nothing declared one: a clipboard entry, a script's line.
pub fn infer(v: &Value) -> Type {
    match v {
        Value::Bool(_) => Type::of("bool"),
        Value::Number(_) => Type::of("number"),
        Value::String(s) => {
            let s = s.trim();
            let kind = if (s.starts_with("https://") || s.starts_with("http://")) && !s.contains(char::is_whitespace) {
                "url"
            } else if (s.starts_with('/') || s.starts_with("~/")) && !s.contains('\n') && s.len() < 1024 {
                let path = if let Some(rest) = s.strip_prefix("~/") { crate::util::home().join(rest) } else { s.into() };
                if path.is_dir() { "folder" } else if path.is_file() {
                    if matches!(path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(), Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "svg")) { "image" } else { "file" }
                } else { "text" }
            } else if s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit()) {
                "color"
            } else {
                "text"
            };
            Type::of(kind)
        }
        Value::Array(items) => Type::list(items.first().map(infer).unwrap_or_else(|| Type::of("any"))),
        Value::Object(m) => {
            if m.get("amount").is_some_and(Value::is_number) && m.get("currency").is_some_and(Value::is_string) {
                Type::of("money")
            } else if m.get("value").is_some_and(Value::is_number) && m.get("unit").is_some_and(Value::is_string) {
                Type::of("quantity")
            } else if m.contains_key("columns") && m.contains_key("rows") {
                Type::of("table")
            } else if m.contains_key("start") && m.contains_key("title") {
                Type::of("event")
            } else {
                Type { kind: "record".into(), fields: m.iter().map(|(k, v)| (k.clone(), infer(v))).collect(), ..Default::default() }
            }
        }
        Value::Null => Type::of("any"),
    }
}

fn group_digits(n: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, n.abs());
    let (whole, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut out = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    let frac = frac.trim_end_matches('0');
    format!("{}{out}{}{frac}", if n < 0.0 { "-" } else { "" }, if frac.is_empty() { "" } else { "." })
}

pub fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { group_digits(n, 0) } else { group_digits(n, if n.abs() < 1.0 { 6 } else { 2 }) }
}

fn currency_sign(code: &str) -> Option<&'static str> {
    Some(match code {
        "USD" => "$", "EUR" => "€", "GBP" => "£", "INR" => "₹", "JPY" => "¥", "CNY" => "¥", "KRW" => "₩", "RUB" => "₽", "BRL" => "R$",
        "CAD" => "CA$", "AUD" => "A$", "CHF" => "CHF ", "ILS" => "₪", "TRY" => "₺", "VND" => "₫", "PHP" => "₱", "BTC" => "₿",
        _ => return None,
    })
}

/// A duration in seconds, as people say it: `12m`, `1h 05m`, `3d`.
pub fn duration(secs: f64) -> String {
    let s = secs.abs().round() as u64;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => if s % 60 == 0 { format!("{}m", s / 60) } else { format!("{}m {:02}s", s / 60, s % 60) },
        3600..86400 => if s % 3600 < 60 { format!("{}h", s / 3600) } else { format!("{}h {:02}m", s / 3600, s % 3600 / 60) },
        _ => format!("{}d", s / 86400),
    }
}

/// A value as one line of text: what a row, a tile or `line` shows.
pub fn display(v: &Value, t: &Type) -> String {
    match (t.kind.as_str(), v) {
        (_, Value::Null) => String::new(),
        ("money", Value::Object(m)) => {
            let (amount, code) = (m.get("amount").and_then(Value::as_f64).unwrap_or(0.0), m.get("currency").and_then(Value::as_str).unwrap_or(""));
            let decimals = if matches!(code, "JPY" | "KRW" | "VND") { 0 } else { 2 };
            let body = format!("{:.*}", decimals, amount.abs());
            let (whole, frac) = body.split_once('.').unwrap_or((&body, ""));
            let whole = group_digits(whole.parse().unwrap_or(0.0), 0);
            let text = if frac.is_empty() { whole } else { format!("{whole}.{frac}") };
            let sign = if amount < 0.0 { "-" } else { "" };
            match currency_sign(code) {
                Some(mark) => format!("{sign}{mark}{text}"),
                None => format!("{sign}{text} {code}"),
            }
        }
        ("quantity", Value::Object(m)) => format!("{} {}", number(m.get("value").and_then(Value::as_f64).unwrap_or(0.0)), m.get("unit").and_then(Value::as_str).unwrap_or("")).trim().to_string(),
        ("percent", Value::Number(n)) => format!("{}%", number(n.as_f64().unwrap_or(0.0))),
        ("duration", Value::Number(n)) => duration(n.as_f64().unwrap_or(0.0)),
        (_, Value::Number(n)) => number(n.as_f64().unwrap_or(0.0)),
        (_, Value::Bool(b)) => if *b { "Yes".into() } else { "No".into() },
        ("markdown" | "text" | "code", Value::String(s)) => s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().trim_start_matches(['#', ' ']).to_string(),
        (_, Value::String(s)) => s.clone(),
        ("event", Value::Object(m)) => m.get("title").and_then(Value::as_str).unwrap_or("Event").to_string(),
        ("list" | "series", Value::Array(a)) => match a.len() {
            1 => "1 item".into(),
            n => format!("{n} items"),
        },
        ("table", Value::Object(m)) => format!("{} rows", m.get("rows").and_then(Value::as_array).map_or(0, Vec::len)),
        (_, Value::Object(m)) => ["title", "name", "label", "text", "value", "id"].iter().find_map(|k| m.get(*k)).map(|x| display(x, &infer(x))).unwrap_or_else(|| format!("{} fields", m.len())),
        (_, Value::Array(a)) => format!("{} items", a.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_fit_values() {
        let input = Type::from_json(&json!({ "amount": { "type": "number" }, "from": { "type": "currency" }, "to": { "type": "currency", "default": "INR" }, "note": { "type": "text", "optional": true } })).unwrap();
        let fitted = input.coerce(json!({ "amount": "1,250.5", "from": "usd" })).unwrap();
        assert_eq!(fitted, json!({ "amount": 1250.5, "from": "USD", "to": "INR" }));
        assert_eq!(input.coerce(json!({ "from": "usd" })).unwrap_err(), "amount: expected number, got nothing");
        assert_eq!(input.coerce(json!({ "amount": [1], "from": "usd" })).unwrap_err(), "amount: expected number, got [1]");
        assert!(Type::from_json(&json!({ "type": "txt" })).unwrap_err().contains("text"));
        assert_eq!(input.json_schema()["required"], json!(["amount", "from"]));
    }

    #[test]
    fn labels_and_luau() {
        let pr = Type { name: "pr".into(), ..Type::record([("title", Type::of("text")), ("url", Type::of("url"))]) };
        let list = Type::list(pr.clone());
        assert_eq!(list.label(), "list<pr>");
        assert_eq!(list.luau(), "{ { title: string, url: string } }");
        assert!(Type::of("text").accepts(&Type::of("url")) && !Type::of("url").accepts(&Type::of("text")));
        assert_eq!(Type::record([("link", Type::of("url")), ("n", Type::of("number").optional())]).slot_for(&Type::of("url")), Some("link"));
    }

    #[test]
    fn values_read_well() {
        assert_eq!(display(&json!({ "amount": 4170.5, "currency": "INR" }), &Type::of("money")), "₹4,170.50");
        assert_eq!(display(&json!({ "amount": -12.0, "currency": "SEK" }), &Type::of("money")), "-12.00 SEK");
        assert_eq!(display(&json!(1234567.0), &Type::of("number")), "1,234,567");
        assert_eq!(display(&json!(3900), &Type::of("duration")), "1h 05m");
        assert_eq!(display(&json!("# Title\nbody"), &Type::of("markdown")), "Title");
        assert_eq!(infer(&json!("https://a.b/c")).kind, "url");
        assert_eq!(infer(&json!({ "amount": 1, "currency": "USD" })).kind, "money");
    }
}
