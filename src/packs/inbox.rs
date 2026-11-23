//! Point and say. On any pack surface a person points at something and says what is wrong in a
//! sentence; the host saves what they were looking at, exactly, to the pack's `inbox/`. The next
//! agent session reads it with `echospot dev inbox`, and can draw the same state again.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Report {
    /// Seconds since 1970.
    pub t: f64,
    pub pack: String,
    /// `ability:prs:panel`, `surface:timer`, `board:morning`.
    pub target: String,
    /// What the person said.
    pub note: String,
    /// The row or block they were on: `List › row 2 "Fix CI"`.
    pub element: String,
    pub args: Value,
    pub query: String,
    /// What was on screen, as blocks: the exact state, to draw again.
    pub blocks: Value,
    /// The same, as text.
    pub tree: String,
    /// What the surface had loaded, by address, and what failed: with the stores, the state
    /// `echospot dev render --inbox` draws the surface in again, through the code as it is now.
    pub loads: serde_json::Map<String, Value>,
    pub failed: serde_json::Map<String, Value>,
    pub stores: serde_json::Map<String, Value>,
    /// An error that was showing, when the report came from "Report to agent".
    pub error: String,
    pub screenshot: String,
    pub resolved: bool,
    /// The folder it lives in; not written.
    #[serde(skip)]
    pub dir: PathBuf,
}

fn stamp(t: f64) -> String {
    use chrono::{Local, TimeZone};
    Local.timestamp_opt(t as i64, 0).single().map(|d| d.format("%Y-%m-%d-%H%M%S").to_string()).unwrap_or_else(|| format!("{t:.0}"))
}

/// The scene a report's state makes: what was loaded, what failed and what the stores held.
pub fn scene(report: &Report, name: &str) -> super::shared::Scene {
    super::shared::Scene {
        name: format!("inbox {name}"), load: report.loads.clone(), errors: report.failed.clone(), stores: report.stores.clone(),
        args: report.args.clone(), query: report.query.clone(), ..Default::default()
    }
}

/// Saves a report into the pack's inbox and returns its folder. `png` is a screenshot to keep
/// beside it.
pub fn save(pack_dir: &Path, mut report: Report, png: Option<&Path>) -> std::io::Result<PathBuf> {
    let dir = pack_dir.join("inbox").join(stamp(report.t));
    std::fs::create_dir_all(&dir)?;
    if let Some(png) = png.filter(|p| p.exists()) {
        std::fs::copy(png, dir.join("screenshot.png"))?;
        report.screenshot = "screenshot.png".into();
    }
    std::fs::write(dir.join("report.json"), serde_json::to_string_pretty(&report).unwrap_or_default())?;
    // What the agent reads first: the sentence, and where it points.
    let mut text = format!("# {}\n\n> {}\n\n- Surface: `{}`\n", report.pack, report.note, report.target);
    if !report.element.is_empty() {
        text.push_str(&format!("- Pointing at: {}\n", report.element));
    }
    if !report.error.is_empty() {
        text.push_str(&format!("- Error showing: {}\n", report.error));
    }
    if !report.query.is_empty() {
        text.push_str(&format!("- Typed: `{}`\n", report.query));
    }
    text.push_str(&format!("\n## What was on screen\n\n```\n{}```\n\nDraw it again: `echospot dev render {} --inbox {}`\n", report.tree, report.target, dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
    std::fs::write(dir.join("REPORT.md"), text)?;
    Ok(dir)
}

/// Every report in a pack's inbox, oldest first.
pub fn list(pack_dir: &Path) -> Vec<Report> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(pack_dir.join("inbox")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.join("report.json").is_file()).collect();
    dirs.sort();
    dirs.into_iter().filter_map(|dir| {
        let mut report: Report = serde_json::from_str(&std::fs::read_to_string(dir.join("report.json")).ok()?).ok()?;
        report.dir = dir;
        Some(report)
    }).collect()
}

/// One report by its folder's name, or the newest when none is named.
pub fn find(pack_dir: &Path, name: &str) -> Option<Report> {
    let all = list(pack_dir);
    if name.is_empty() {
        return all.into_iter().filter(|r| !r.resolved).next_back();
    }
    all.into_iter().find(|r| r.dir.file_name().is_some_and(|n| n.to_string_lossy() == name))
}

/// Marks a report dealt with. It stays on disk, out of the list an agent is shown.
pub fn resolve(report: &Report) -> std::io::Result<()> {
    let mut done = report.clone();
    done.resolved = true;
    std::fs::write(report.dir.join("report.json"), serde_json::to_string_pretty(&done).unwrap_or_default())
}

/// What a person was pointing at, in words: the selected row of a list, or the block itself.
pub fn element(blocks: &Value, selected: usize) -> String {
    let kind = super::blocks::kind(blocks);
    match super::blocks::items(blocks, "rows").get(selected) {
        Some(row) => format!("{kind} › row {} {:?}", selected + 1, super::blocks::show(row.get("title").unwrap_or(&Value::Null), None)),
        None => kind.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_report_keeps_what_was_on_screen() {
        let dir = std::env::temp_dir().join(format!("echospot-inbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let blocks = json!({ "_ui": "List", "rows": [{ "title": "Fix CI" }, { "title": "Docs" }] });
        let report = Report { t: 1_794_700_000.0, pack: "github".into(), target: "ability:prs:panel".into(), note: "show the repo name here".into(), element: element(&blocks, 1), tree: crate::packs::blocks::tree(&blocks), blocks, ..Default::default() };
        assert_eq!(report.element, "List › row 2 \"Docs\"");
        let saved = save(&dir, report, None).unwrap();
        assert!(std::fs::read_to_string(saved.join("REPORT.md")).unwrap().contains("> show the repo name here"));
        let found = find(&dir, "").unwrap();
        assert_eq!((found.note.as_str(), found.blocks["rows"][1]["title"].as_str()), ("show the repo name here", Some("Docs")));
        resolve(&found).unwrap();
        assert!(find(&dir, "").is_none());
        assert_eq!(list(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
