//! Errors that teach: a code, where, what was expected, the nearest valid names, and a command
//! to learn more. Every failure a pack or an agent can meet is one of these.

use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PackError {
    pub code: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub at: String,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nearest: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub learn: String,
}

pub type Result<T> = std::result::Result<T, PackError>;

/// The codes, and what each means. `echospot dev docs errors` prints this table.
pub const CODES: &[(&str, &str)] = &[
    ("E0100", "pack.toml is missing or isn't valid TOML"),
    ("E0101", "pack.toml has a field with the wrong shape"),
    ("E0102", "the pack's id is reserved, or isn't lowercase letters, digits and dashes"),
    ("E0103", "the pack asks for an API version this EchoSpot doesn't speak"),
    ("E0200", "a Luau file didn't compile"),
    ("E0201", "init.luau didn't return echo.pack { … }"),
    ("E0202", "a declaration is missing a field or has one of the wrong type"),
    ("E0203", "a Luau error while the pack ran"),
    ("E0204", "an entry into the pack ran past its time limit"),
    ("E0300", "no ability at that address"),
    ("E0301", "the input doesn't match the ability's schema"),
    ("E0302", "the output doesn't match the ability's declared type"),
    ("E0310", "a read ability called an effect"),
    ("E0311", "an effect has no undo, and calls an effect without one"),
    ("E0400", "the pack has no grant for this"),
    ("E0401", "the address isn't in the pack's `calls`"),
    ("E0402", "the host isn't one the pack declared"),
    ("E0403", "the path is outside what the pack may touch"),
    ("E0404", "the program or its arguments don't match a declared template"),
    ("E0405", "the secret isn't declared, or not for this host"),
    ("E0406", "the pack's AI budget for this month is spent"),
    ("E0412", "no such name"),
    ("E0500", "the network request failed"),
    ("E0501", "the response was larger than the cap"),
    ("E0502", "a redirect led to a host the pack didn't declare"),
    ("E0510", "the program failed to run"),
    ("E0520", "AI isn't set up, or the provider failed"),
    ("E0600", "the icon doesn't follow EchoSpot's icon spec"),
    ("E0700", "a design rule was broken"),
    ("E0800", "a test failed"),
    ("E0900", "the call was cancelled or timed out"),
];

impl PackError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, at: String::new(), message: message.into(), nearest: Vec::new(), learn: String::new() }
    }
    pub fn at(mut self, at: impl Into<String>) -> Self {
        self.at = at.into();
        self
    }
    pub fn learn(mut self, command: impl Into<String>) -> Self {
        self.learn = command.into();
        self
    }
    /// Names close to `wanted` among `known`, closest first.
    pub fn nearest<'a>(mut self, wanted: &str, known: impl IntoIterator<Item = &'a str>) -> Self {
        self.nearest = nearest(wanted, known);
        self
    }
    pub fn json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({ "code": self.code, "message": self.message }))
    }
    /// Reads an error back from what `json` wrote, as one crosses a VM or a process.
    pub fn from_json(v: &Value) -> Self {
        let code = v.get("code").and_then(Value::as_str).and_then(|c| CODES.iter().find(|(k, _)| *k == c)).map(|(k, _)| *k).unwrap_or("E0203");
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Self {
            code, at: text("at"), message: if v.is_string() { v.as_str().unwrap_or("").to_string() } else { text("message") },
            nearest: v.get("nearest").and_then(Value::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default(),
            learn: text("learn"),
        }
    }
    /// A Luau error message, with its `file:line:` lifted into `at`.
    pub fn luau(message: &str) -> Self {
        let first = message.lines().next().unwrap_or(message);
        let first = first.strip_prefix("runtime error: ").unwrap_or(first);
        // An error raised by the host carries a whole PackError as JSON.
        if let Some(at) = message.find("{\"code\":\"E")
            && let Some(v) = message[at..].lines().next().and_then(|l| serde_json::from_str::<Value>(l).ok()) {
            let mut e = Self::from_json(&v);
            // Where the pack's own code was when the host raised it.
            let lead = message[..at].trim().trim_end_matches(':');
            if e.at.is_empty() && let Some((file, line)) = lead.rsplit_once(':') && line.trim().parse::<u32>().is_ok() {
                e.at = format!("{}:{}", file.rsplit(' ').next().unwrap_or(file).trim_start_matches(['@', '=']), line.trim());
            }
            return e;
        }
        let code = if first.contains("time limit") || first.contains("timed out") || first.contains("interrupted") { "E0204" } else { "E0203" };
        let mut parts = first.splitn(3, ':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(file), Some(line), Some(rest)) if line.trim().parse::<u32>().is_ok() => {
                let file = file.trim().trim_start_matches(['@', '=']).trim_start_matches("[string \"").trim_end_matches("\"]");
                Self::new(code, rest.trim()).at(format!("{file}:{}", line.trim()))
            }
            _ => Self::new(code, first.trim()),
        }
    }
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)?;
        if !self.at.is_empty() {
            write!(f, " at {}", self.at)?;
        }
        write!(f, ": {}", self.message)?;
        if !self.nearest.is_empty() {
            write!(f, " (did you mean {}?)", self.nearest.join(", "))?;
        }
        if !self.learn.is_empty() {
            write!(f, "\n  learn: {}", self.learn)?;
        }
        Ok(())
    }
}

fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.to_lowercase().chars().collect(), b.to_lowercase().chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (prev + usize::from(ca != cb)).min(row[j] + 1).min(row[j + 1] + 1);
            prev = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

/// Up to three names within reach of `wanted`: a typo, a missing suffix, the same word inside.
pub fn nearest<'a>(wanted: &str, known: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let lower = wanted.to_lowercase();
    let mut scored: Vec<(usize, &str)> = known.into_iter().filter_map(|name| {
        let d = distance(wanted, name);
        let inside = !lower.is_empty() && (name.to_lowercase().contains(&lower) || lower.contains(&name.to_lowercase()));
        (d <= (wanted.chars().count() / 2).max(2) || inside).then_some((if inside { d.min(2) } else { d }, name))
    }).collect();
    scored.sort();
    scored.dedup();
    scored.into_iter().take(3).map(|(_, n)| n.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_finds_typos_and_prefixes() {
        assert_eq!(nearest("Table2", ["Table", "Grid", "KeyValue", "List"])[0], "Table");
        assert!(nearest("zzz", ["Table", "Grid"]).is_empty());
    }

    #[test]
    fn luau_errors_carry_their_place() {
        let e = PackError::luau("init.luau:31: attempt to index nil with 'x'\nstack traceback: …");
        assert_eq!((e.at.as_str(), e.code), ("init.luau:31", "E0203"));
        let round = PackError::from_json(&PackError::new("E0400", "no grant").learn("echospot dev grants").json());
        assert_eq!(round.code, "E0400");
        assert_eq!(PackError::luau(&format!("init.luau:3: {}", round.json())).learn, "echospot dev grants");
    }
}
