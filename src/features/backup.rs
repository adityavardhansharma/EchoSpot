//! Backup and restore: everything a person has made in EchoSpot, as one `.tar.gz`.
//!
//! A backup holds settings, snippets, quicklinks, aliases and hotkeys, AI actions and presets,
//! script commands, notes and favorites. Secrets (API keys, MCP server environments, extension
//! passwords) are left out unless asked for.

use std::path::{Path, PathBuf};

use rune::Tx;
use serde_json::Value;

use crate::app::{App, Tone};
use crate::features::forms::{self, Field, FieldKind, Form, Target};
use crate::model::*;
use crate::util;

/// Files and folders of the config directory a backup carries, and whether each may hold a secret.
const CONFIG: [(&str, bool); 9] = [
    ("settings.json", false), ("snippets.json", false), ("quicklinks.json", false), ("bindings.json", false), ("ai.json", false),
    ("commands.json", false), ("background.json", false), ("scripts", false), ("mcp.json", true),
];
const SECRET_DIRS: [&str; 1] = ["extensions"];
const DATA: [&str; 2] = ["notes", "usage.json"];

fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else if from.is_file() {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from, to)?;
    }
    Ok(())
}

fn staging(tag: &str) -> PathBuf {
    util::runtime_dir().join(format!("echospot-{tag}-{}-{}", std::process::id(), util::now_secs()))
}

/// Writes a backup to `target`. With `secrets`, API keys and other credentials go in too.
pub fn export(target: &Path, secrets: bool) -> Result<usize, String> {
    let stage = staging("backup");
    let root = stage.join("echospot-backup");
    let io = |e: std::io::Error| format!("Could not write the backup: {e}");
    let (config, data) = (util::config_dir(), util::data_dir());
    let mut count = 0;
    for (name, secret) in CONFIG {
        let from = config.join(name);
        if !from.exists() || (secret && !secrets) {
            continue;
        }
        copy_all(&from, &root.join("config").join(name)).map_err(io)?;
        count += 1;
    }
    if secrets {
        for name in SECRET_DIRS {
            if config.join(name).exists() {
                copy_all(&config.join(name), &root.join("config").join(name)).map_err(io)?;
                count += 1;
            }
        }
    } else if let Ok(text) = std::fs::read_to_string(root.join("config/settings.json"))
        && let Ok(mut settings) = serde_json::from_str::<Value>(&text) {
        // The settings travel; the keys in them do not.
        if let Some(ai) = settings.get_mut("ai").and_then(Value::as_object_mut) {
            ai.remove("keys");
            ai.remove("api_key");
        }
        std::fs::write(root.join("config/settings.json"), serde_json::to_string_pretty(&settings).unwrap_or(text)).map_err(io)?;
    }
    for name in DATA {
        if data.join(name).exists() {
            copy_all(&data.join(name), &root.join("data").join(name)).map_err(io)?;
            count += 1;
        }
    }
    let manifest = serde_json::json!({ "app": "echospot", "version": env!("CARGO_PKG_VERSION"), "created": util::now_secs(), "secrets": secrets });
    std::fs::create_dir_all(&root).map_err(io)?;
    std::fs::write(root.join("backup.json"), manifest.to_string()).map_err(io)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let ok = std::process::Command::new("tar").arg("-czf").arg(target).arg("-C").arg(&stage).arg("echospot-backup")
        .status().is_ok_and(|s| s.success());
    let _ = std::fs::remove_dir_all(&stage);
    if !ok {
        return Err("Could not write the backup: tar failed.".into());
    }
    if secrets {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o600));
    }
    Ok(count)
}

/// Puts a backup's contents in place. What is there now is itself backed up first, keys and all,
/// under `~/.local/share/echospot/backups`, so a restore can be undone.
pub fn restore(archive: &Path) -> Result<usize, String> {
    if !archive.is_file() {
        return Err(format!("{} is not a file.", util::tilde(archive)));
    }
    let stage = staging("restore");
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let extracted = std::process::Command::new("tar").arg("-xzf").arg(archive).arg("-C").arg(&stage)
        .stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
    let root = stage.join("echospot-backup");
    if !extracted || !root.join("backup.json").is_file() {
        let _ = std::fs::remove_dir_all(&stage);
        return Err("That file is not an EchoSpot backup.".into());
    }
    let safety = util::data_dir().join("backups").join(format!("before-restore-{}.tar.gz", util::now_secs()));
    export(&safety, true)?;
    let io = |e: std::io::Error| format!("Could not restore: {e}");
    let (config, data) = (util::config_dir(), util::data_dir());
    let mut count = 0;
    // Only what a backup is known to hold is taken from the archive.
    for name in CONFIG.iter().map(|(n, _)| *n).chain(SECRET_DIRS) {
        let from = root.join("config").join(name);
        if !from.exists() {
            continue;
        }
        if name == "settings.json" {
            // A backup made without keys must not wipe the ones in use.
            let mut incoming: Value = util::read_json(&from);
            let current: Value = util::read_json(&config.join(name));
            if incoming.get("ai").and_then(|ai| ai.get("keys")).is_none()
                && let (Some(ai), Some(keys)) = (incoming.get_mut("ai").and_then(Value::as_object_mut), current.get("ai").and_then(|ai| ai.get("keys"))) {
                ai.insert("keys".into(), keys.clone());
            }
            util::write_json(&config.join(name), &incoming);
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(config.join(name), std::fs::Permissions::from_mode(0o600));
        } else {
            copy_all(&from, &config.join(name)).map_err(io)?;
        }
        count += 1;
    }
    for name in DATA {
        let from = root.join("data").join(name);
        if from.exists() {
            copy_all(&from, &data.join(name)).map_err(io)?;
            count += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&stage);
    Ok(count)
}

fn default_target() -> PathBuf {
    util::home().join(format!("echospot-backup-{}.tar.gz", chrono::Local::now().format("%Y-%m-%d")))
}

fn export_form() -> Form {
    Form::new("Export Backup", "Export", vec![
        Field::new("path", "Save As", FieldKind::Text).value(&util::tilde(&default_target())).required(),
        Field::new("secrets", "Secrets", FieldKind::Checkbox("Include API keys and extension passwords".into()))
            .info("Settings, snippets, quicklinks, aliases and hotkeys, AI actions and presets, scripts, notes and favorites are always included."),
    ], Target::BackupExport)
}

fn import_form() -> Form {
    Form::new("Import Backup", "Import", vec![
        Field::new("path", "Backup File", FieldKind::Text).placeholder("~/echospot-backup-2026-11-12.tar.gz").required()
            .info("What the backup holds replaces what is here. Everything here now is saved first, in ~/.local/share/echospot/backups."),
    ], Target::BackupImport)
}

fn expand(path: &str) -> PathBuf {
    match path.trim().strip_prefix("~/") {
        Some(rest) => util::home().join(rest),
        None => PathBuf::from(path.trim()),
    }
}

pub fn perform(app: &mut App, op: BackupOp, tx: &mut Tx) {
    forms::open(app, match op { BackupOp::Export => export_form(), BackupOp::Import => import_form() }, tx);
}

pub fn submit_export(app: &mut App, tx: &mut Tx) {
    let Some(form) = &app.form else { return };
    let (target, secrets) = (expand(&form.get("path")), form.get("secrets") == "true");
    app.pop(tx);
    match export(&target, secrets) {
        Ok(_) => app.toast(format!("Backup saved to {}", util::tilde(&target)), Tone::Success),
        Err(e) => app.toast(e, Tone::Danger),
    }
}

pub fn submit_import(app: &mut App, tx: &mut Tx) {
    let Some(form) = &app.form else { return };
    let archive = expand(&form.get("path"));
    match restore(&archive) {
        Ok(count) => {
            app.pop(tx);
            app.reload_stores();
            app.toast(format!("Restored {count} item{} from the backup", if count == 1 { "" } else { "s" }), Tone::Success);
        }
        Err(e) => {
            if let Some(field) = app.form.as_mut().and_then(|f| f.fields.iter_mut().find(|f| f.id == "path")) {
                field.error = e;
            }
        }
    }
}

/// `echospot backup [file] [--with-keys]` and `echospot restore <file>`.
pub fn cli(command: &str, args: &[String]) {
    let secrets = args.iter().any(|a| a == "--with-keys");
    let path = args.iter().find(|a| !a.starts_with("--")).map(|p| expand(p));
    match command {
        "backup" => {
            let target = path.unwrap_or_else(default_target);
            match export(&target, secrets) {
                Ok(count) => println!("{count} items saved to {}{}", target.display(), if secrets { " (with keys: keep it private)" } else { " (without API keys; add --with-keys to include them)" }),
                Err(e) => eprintln!("{e}"),
            }
        }
        _ => match path {
            Some(archive) => match restore(&archive) {
                Ok(count) => {
                    println!("{count} items restored. What was there before is in {}.", util::data_dir().join("backups").display());
                    if crate::ipc::send("reload") {
                        println!("The running EchoSpot has reloaded.");
                    }
                }
                Err(e) => eprintln!("{e}"),
            },
            None => eprintln!("echospot restore <backup.tar.gz>"),
        },
    }
}
