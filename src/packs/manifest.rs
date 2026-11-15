//! `pack.toml`: a pack's identity and the permissions it asks for, read without running any of
//! its code, so the host can show them first.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::error::{PackError, Result};

/// Ids that belong to the host's own abilities.
pub const RESERVED: &[&str] = &[
    "echo", "calc", "clipboard", "notes", "windows", "files", "calendar", "ai", "system", "snippets", "quicklinks", "http", "run",
    "scripts", "raycast", "apps", "emoji", "timeline", "stores",
];

pub const TINTS: &[&str] = &["violet", "indigo", "blue", "cyan", "teal", "green", "yellow", "orange", "red", "pink", "graphite"];

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub api: u32,
    pub tint: String,
    pub description: String,
    pub permissions: Permissions,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Permissions {
    /// Hosts the pack may speak HTTPS to.
    pub net: Vec<String>,
    /// Secret name → the hosts it may be sent to.
    pub secrets: BTreeMap<String, Vec<String>>,
    /// `none`, `write` or `read` (which includes write).
    pub clipboard: String,
    /// Path globs outside the pack's own data folder.
    pub files: Vec<String>,
    pub run: Vec<RunTemplate>,
    pub ai: Option<AiBudget>,
    /// Abilities and stores of other packs, and built-ins that aren't free.
    pub calls: Vec<String>,
    /// Schedules and event triggers.
    pub background: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RunTemplate {
    pub bin: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AiBudget {
    pub budget_usd_month: f64,
}

/// One thing a pack asks for, as the grants screen lists it and the grants file records it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
pub struct Ask {
    /// `net`, `secret`, `clipboard`, `files`, `run`, `ai`, `calls`, `background`.
    pub kind: String,
    pub what: String,
}

impl Ask {
    pub fn new(kind: &str, what: impl Into<String>) -> Self {
        Self { kind: kind.into(), what: what.into() }
    }
    /// The sentence a person reads before granting.
    pub fn sentence(&self) -> String {
        match self.kind.as_str() {
            "net" => format!("Talk to {} over HTTPS", self.what),
            "secret" => {
                let (name, hosts) = self.what.split_once(" → ").unwrap_or((&self.what, ""));
                format!("Send your “{name}” secret to {hosts}")
            }
            "clipboard" if self.what == "read" => "Read and write your clipboard, and its history".into(),
            "clipboard" => "Write to your clipboard".into(),
            "files" => format!("Read and write files matching {}", self.what),
            "run" => format!("Run {}", self.what),
            "ai" => format!("Use your AI provider, up to ${} a month", self.what),
            "calls" => format!("Use {}", self.what),
            "background" => "Run in the background, on schedules and events".into(),
            _ => self.what.clone(),
        }
    }
}

impl Permissions {
    /// Everything asked for, one grantable line each, in a stable order.
    pub fn asks(&self) -> Vec<Ask> {
        let mut out: Vec<Ask> = self.net.iter().map(|h| Ask::new("net", h.to_lowercase())).collect();
        out.extend(self.secrets.iter().map(|(name, hosts)| Ask::new("secret", format!("{name} → {}", hosts.join(", ")))));
        if matches!(self.clipboard.as_str(), "read" | "write") {
            out.push(Ask::new("clipboard", &self.clipboard));
        }
        out.extend(self.files.iter().map(|g| Ask::new("files", g)));
        out.extend(self.run.iter().map(|r| Ask::new("run", std::iter::once(r.bin.clone()).chain(r.args.iter().cloned()).collect::<Vec<_>>().join(" "))));
        if let Some(ai) = &self.ai {
            out.push(Ask::new("ai", format!("{:.2}", ai.budget_usd_month)));
        }
        out.extend(self.calls.iter().map(|c| Ask::new("calls", c)));
        if self.background {
            out.push(Ask::new("background", "on"));
        }
        out
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && id.starts_with(|c: char| c.is_ascii_lowercase())
}

/// A host as `net` may name it: a bare domain, no scheme, port, path or wildcard.
fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.contains('.') && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') && !host.starts_with('.') && !host.ends_with('.')
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self> {
        let learn = "echospot dev docs pack.toml";
        let m: Manifest = toml::from_str(text).map_err(|e| {
            let at = e.span().map(|s| format!("pack.toml:{}", text[..s.start.min(text.len())].lines().count().max(1))).unwrap_or_else(|| "pack.toml".into());
            let message = e.message().to_string();
            let code = if message.contains("unknown field") || message.contains("invalid type") || message.contains("missing field") { "E0101" } else { "E0100" };
            PackError::new(code, message).at(at).learn(learn)
        })?;
        m.validate()?;
        Ok(m)
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(dir.join("pack.toml"))
            .map_err(|e| PackError::new("E0100", format!("can't read pack.toml: {e}")).at(dir.join("pack.toml").display().to_string()).learn("echospot dev new <id>"))?;
        Self::parse(&text)
    }

    fn validate(&self) -> Result<()> {
        let bad = |code, message: String| Err(PackError::new(code, message).at("pack.toml").learn("echospot dev docs pack.toml"));
        if !valid_id(&self.id) {
            return bad("E0102", format!("id {:?} must be lowercase letters, digits and dashes, starting with a letter", self.id));
        }
        if RESERVED.contains(&self.id.as_str()) {
            return bad("E0102", format!("id {:?} is reserved for EchoSpot's own abilities", self.id));
        }
        if self.api == 0 || self.api > super::API || self.api + 1 < super::API {
            return bad("E0103", format!("api = {} isn't supported; this EchoSpot speaks api {}", self.api, super::API));
        }
        if !self.tint.is_empty() && !TINTS.contains(&self.tint.as_str()) {
            return Err(PackError::new("E0101", format!("tint {:?} isn't a palette tint", self.tint)).at("pack.toml").nearest(&self.tint, TINTS.iter().copied()));
        }
        let p = &self.permissions;
        if !matches!(p.clipboard.as_str(), "" | "none" | "write" | "read") {
            return bad("E0101", format!("clipboard = {:?} must be \"none\", \"write\" or \"read\"", p.clipboard));
        }
        for host in p.net.iter().chain(p.secrets.values().flatten()) {
            if !valid_host(host) {
                return bad("E0101", format!("{host:?} isn't a host name; name hosts only, such as \"api.github.com\" (https is implied)"));
            }
        }
        for (name, hosts) in &p.secrets {
            if let Some(host) = hosts.iter().find(|h| !p.net.iter().any(|n| n.eq_ignore_ascii_case(h))) {
                return bad("E0101", format!("secret {name:?} names {host:?}, which isn't in net"));
            }
        }
        for r in &p.run {
            if !r.bin.starts_with('/') {
                return bad("E0101", format!("run.bin {:?} must be an absolute path; there is no shell and no PATH lookup", r.bin));
            }
        }
        if p.ai.as_ref().is_some_and(|a| !(a.budget_usd_month >= 0.0)) {
            return bad("E0101", "ai.budget_usd_month must be zero or more".into());
        }
        for call in &p.calls {
            if !call.contains('.') {
                return bad("E0101", format!("calls entry {call:?} must be an address such as \"scripts.pi\""));
            }
        }
        Ok(())
    }

    pub fn display_name(&self) -> &str {
        if self.name.is_empty() { &self.id } else { &self.name }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GITHUB: &str = r#"
id = "github"
name = "GitHub"
api = 1
tint = "graphite"

[permissions]
net = ["api.github.com"]
secrets = { github = ["api.github.com"] }
clipboard = "write"
files = ["~/Projects/**"]
run = [{ bin = "/usr/bin/gh", args = ["pr", "list", "--json", "{fields}"] }]
ai = { budget_usd_month = 2.00 }
calls = ["scripts.pi", "calendar.next"]
background = true
"#;

    #[test]
    fn the_design_example_parses() {
        let m = Manifest::parse(GITHUB).unwrap();
        assert_eq!(m.id, "github");
        let asks = m.permissions.asks();
        assert_eq!(asks.len(), 9);
        assert_eq!(asks[0].sentence(), "Talk to api.github.com over HTTPS");
        assert!(asks.iter().any(|a| a.sentence() == "Run /usr/bin/gh pr list --json {fields}"));
    }

    #[test]
    fn bad_manifests_say_why() {
        assert_eq!(Manifest::parse("id = \"calc\"\napi = 1").unwrap_err().code, "E0102");
        assert_eq!(Manifest::parse("id = \"x\"\napi = 9").unwrap_err().code, "E0103");
        assert_eq!(Manifest::parse("id = \"x\"\napi = 1\nfoo = 2").unwrap_err().code, "E0101");
        assert_eq!(Manifest::parse("id = \"x\"\napi = 1\n[permissions]\nnet = [\"https://a.com/x\"]").unwrap_err().code, "E0101");
        assert_eq!(Manifest::parse("id = \"x\"\napi = 1\n[permissions]\nrun = [{ bin = \"gh\" }]").unwrap_err().code, "E0101");
        assert_eq!(Manifest::parse("id = ").unwrap_err().code, "E0100");
        assert_eq!(Manifest::parse("id = \"x\"\napi = 1\ntint = \"gren\"").unwrap_err().nearest[0], "green");
    }
}
