//! The audit log: every broker call that isn't a free read, one JSON line each. Settings and
//! `echospot dev logs` read it, and the trust labels are facts counted from it.

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Entry {
    /// Seconds since 1970.
    pub t: f64,
    /// The pack that made the call; `user`, `ai` or `mcp` when no pack did.
    pub pack: String,
    pub address: String,
    pub kind: String,
    /// A short summary of the arguments. Secrets never reach here: packs name them, the host holds them.
    pub args: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// Bytes of the result.
    pub bytes: usize,
    pub ms: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub usd: f64,
    /// The host a network call reached.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dry: bool,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

pub fn path() -> PathBuf {
    super::state_dir().join("audit.jsonl")
}

pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Past this the oldest half is dropped.
const MAX_BYTES: u64 = 8 << 20;

pub fn append(entry: &Entry) {
    let path = path();
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES)
        && let Ok(text) = std::fs::read_to_string(&path) {
        let lines: Vec<&str> = text.lines().collect();
        let _ = std::fs::write(&path, lines[lines.len() / 2..].join("\n") + "\n");
    }
    let Ok(line) = serde_json::to_string(entry) else { return };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{line}");
    }
}

pub fn read() -> Vec<Entry> {
    std::fs::read_to_string(path()).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// The last `n` entries, a pack's own when `pack` names one.
pub fn tail(pack: Option<&str>, n: usize) -> Vec<Entry> {
    let mut all: Vec<Entry> = read().into_iter().filter(|e| pack.is_none_or(|p| e.pack == p)).collect();
    let from = all.len().saturating_sub(n);
    all.drain(..from);
    all
}

/// The first second of the current month, local time.
pub fn month_start() -> f64 {
    use chrono::{Datelike, Local, TimeZone};
    let now = Local::now();
    Local.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0).single().map(|d| d.timestamp() as f64).unwrap_or(0.0)
}

/// What a pack has spent on AI since the month began.
pub fn spend_this_month(pack: &str) -> f64 {
    let since = month_start();
    read().iter().filter(|e| e.pack == pack && e.t >= since).map(|e| e.usd).sum()
}

pub fn line(e: &Entry) -> String {
    use chrono::{Local, TimeZone};
    let when = Local.timestamp_opt(e.t as i64, 0).single().map(|d| d.format("%m-%d %H:%M:%S").to_string()).unwrap_or_default();
    let mut out = format!("{when}  {:<12} {:<22} {} {}", e.pack, e.address, if e.ok { "ok " } else { "ERR" }, e.args);
    if e.usd > 0.0 {
        out.push_str(&format!("  ${:.4}", e.usd));
    }
    if e.dry {
        out.push_str("  (dry)");
    }
    if !e.error.is_empty() {
        out.push_str(&format!("  {}", e.error));
    }
    out
}
