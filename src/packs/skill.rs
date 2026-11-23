//! `echospot agent install`: what a coding agent needs to know to build a pack, kept short on
//! purpose. It says when to use it, the loop, and the rules that can't be looked up; everything
//! else is `echospot dev docs`, printed for the version installed, so this never goes stale.

use std::path::PathBuf;

use crate::util;

pub const SKILL: &str = r#"---
name: echospot
description: Build, change or fix an EchoSpot pack: a command, extension, dashboard, overlay or automation for the EchoSpot command palette on this machine. Use when the user says "make me a command that…", "add … to EchoSpot", "an EchoSpot extension/pack/board/widget for…", or wants an existing pack changed. Triggers: echospot, pack, command palette, launcher command, board, dashboard tile, overlay, quick action.
---

# EchoSpot packs

A pack is a folder of Luau under `~/.config/echospot/packs/<id>/`. EchoSpot reloads it on save.
It declares **abilities** (typed functions with an address), **surfaces**, **triggers** and
**stores**. Everything that touches the world is `cx:call(address, input)`, checked by a broker.

## The loop

1. `echospot dev inbox` — read what the human pointed at, if anything. Read the pack's `INTENT.md`.
2. `echospot dev new <id> --from currency|github|board|overlay|blank` — a pack that already loads.
   For heavy compute or a Rust crate, `--from rust`: abilities in Rust, UI in Luau (`echospot dev docs rust`).
3. Edit `init.luau`. Find what exists first: `echospot dev api search <words>`, `echospot dev api show <name>`.
4. `echospot dev check` — fix every error it prints. Each has a code, a place and a `learn` command.
5. `echospot dev render <ability>` — **open the PNG and look at it**. Read the text tree beside it.
6. `echospot dev check --design` — every scene, the host's stress scenes included, measured.
7. `echospot dev run <address> --json '{…}' --dry` — see what an effect would do before doing it.
8. `echospot dev test` — write `tests/*.luau` for what matters; mock the world with `t:mock`.
9. Add to `INTENT.md`: what was asked in the human's words, what you decided and why, known limits.

`echospot dev docs` lists every topic. `echospot dev docs start` is this loop in full.

## Design rules

- Say **what**, not how: return `ui.List { items = …, title = "name" }`. Never sizes, colours or spacing.
- Write no loading, empty or error drawing. Give a component the Resource from `cx:load(…)` and an `empty = { title, hint }`.
- Mean something with `intent` (`urgent`, `live`, `success`, `new`, `warning`, `muted`); the Look decides how it shows. Mark few things, or none stand out.
- Titles in Title Case, two or three words for an action. An accessory is a word or two.
- Pick a tint by name (`tint = "green"`); never a hex colour.

## Icon rules

Every ability gets `icons/<name>.svg`: 24×24 viewBox, `fill="none" stroke="currentColor"
stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"`, drawn inside 2–22, at most
8 shapes, fills only for dots, no text or transforms. Draw the idea, not a generic shape.
`echospot dev icon check icons/<name>.svg`, then look at the sheet it writes.

## Safety rules

- Ask for the least in `pack.toml` `[permissions]`. A pack with none runs with no grant screen.
- **You cannot grant a permission.** Only the human can, in EchoSpot's window (Review Permissions).
  When `echospot dev grants` shows something missing, tell them what it is and why, and stop.
- Never put a token or a key in a pack. Name a secret (`auth = "github"`); the human stores it in EchoSpot.
- `kind = "read"` changes nothing. `kind = "effect"` needs an `undo`. `kind = "final"` can't be undone and asks first. Be honest about which one it is.
- Never work around the sandbox: no shelling out through a script to dodge `run`, no writing outside the pack's folder.
"#;

pub const AGENTS_BEGIN: &str = "<!-- echospot:begin -->";
pub const AGENTS_END: &str = "<!-- echospot:end -->";

/// The same guidance as a section of an `AGENTS.md`.
pub fn agents_section() -> String {
    let body = SKILL.splitn(3, "---").nth(2).unwrap_or(SKILL).trim();
    format!("{AGENTS_BEGIN}\n{}\n{AGENTS_END}\n", body.replacen("# EchoSpot packs", "## EchoSpot packs", 1).replace("\n## ", "\n### "))
}

/// Puts the section into an `AGENTS.md`, replacing an older copy of itself.
pub fn merge_agents(existing: &str) -> String {
    let section = agents_section();
    match (existing.find(AGENTS_BEGIN), existing.find(AGENTS_END)) {
        (Some(start), Some(end)) if end > start => format!("{}{}{}", &existing[..start], section.trim_end(), &existing[end + AGENTS_END.len()..]),
        _ if existing.trim().is_empty() => section,
        _ => format!("{}\n\n{section}", existing.trim_end()),
    }
}

pub fn skill_path() -> PathBuf {
    util::home().join(".claude").join("skills").join("echospot").join("SKILL.md")
}

/// Writes the skill, and the section into each `AGENTS.md` named. Returns what was written.
pub fn install(agents_md: &[PathBuf]) -> std::io::Result<Vec<PathBuf>> {
    let mut wrote = Vec::new();
    let skill = skill_path();
    if let Some(parent) = skill.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&skill, SKILL)?;
    wrote.push(skill);
    for path in agents_md {
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, merge_agents(&existing))?;
        wrote.push(path.clone());
    }
    Ok(wrote)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_stays_short_and_names_real_commands() {
        assert!(SKILL.lines().count() < 70, "the skill is meant to be short: {} lines", SKILL.lines().count());
        // Every command it names is one the docs list, so the two can't drift apart.
        for line in SKILL.lines() {
            let mut rest = line;
            while let Some(at) = rest.find("`echospot dev ") {
                let command: String = rest[at + 1..].chars().take_while(|c| *c != '`').collect();
                let verb = command.split_whitespace().nth(2).unwrap_or("");
                assert!(crate::packs::docs::COMMANDS.iter().any(|(c, _)| c.split_whitespace().nth(2) == Some(verb)), "`{command}` isn't a documented command");
                rest = &rest[at + 14..];
            }
        }
    }

    #[test]
    fn the_agents_section_replaces_itself() {
        let first = merge_agents("# My project\n\nSome notes.\n");
        assert!(first.starts_with("# My project") && first.contains("### The loop"));
        let second = merge_agents(&first.replace("The loop", "An old loop"));
        assert_eq!(second.matches(AGENTS_BEGIN).count(), 1);
        assert!(second.contains("### The loop") && !second.contains("An old loop"));
        assert!(merge_agents("").starts_with(AGENTS_BEGIN));
    }
}
