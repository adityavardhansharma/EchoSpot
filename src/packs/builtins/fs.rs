//! `files.read`, `files.write`, `files.list`: the disk. A pack always has its own data folder;
//! anything else needs a glob it declared and was granted, or a path the user picked for it in
//! EchoSpot's own picker (`files.pick`), where the choice is the permission.

use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use super::{Ctx, Done, Gate, Run, Spec, text};
use crate::packs::error::{PackError, Result};
use crate::packs::index::Kind;
use crate::util;

const MAX_READ: u64 = 10 << 20;

/// Whether `path` matches `pattern`: `*` and `?` stay inside one folder, `**` crosses them.
pub fn glob(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some(b'*') if p.get(1) == Some(&b'*') => {
                let rest = if p.get(2) == Some(&b'/') { &p[3..] } else { &p[2..] };
                (0..=s.len()).any(|i| (i == 0 || p.get(2) != Some(&b'/') || s[i - 1] == b'/' || i == s.len()) && go(rest, &s[i..])) || go(&p[2..], s)
            }
            Some(b'*') => (0..=s.len()).take_while(|i| *i == 0 || s[i - 1] != b'/').any(|i| go(&p[1..], &s[i..])),
            Some(b'?') => s.first().is_some_and(|c| *c != b'/') && go(&p[1..], &s[1..]),
            Some(c) => s.first() == Some(c) && go(&p[1..], &s[1..]),
        }
    }
    go(pattern.as_bytes(), path.as_bytes())
}

pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => util::home().join(rest),
        None if path == "~" => util::home(),
        None => PathBuf::from(path),
    }
}

/// The real path a name leads to: symlinks followed, `..` gone. A file that doesn't exist yet
/// resolves through its folder, so a link can't lead a write somewhere else.
fn real(path: &Path) -> Result<PathBuf> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(PackError::new("E0403", format!("{} goes up with \"..\"; name the path directly", path.display())));
    }
    if let Ok(real) = path.canonicalize() {
        return Ok(real);
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else { return Err(PackError::new("E0403", format!("{} isn't a path", path.display()))) };
    let parent = parent.canonicalize().map_err(|_| PackError::new("E0403", format!("{} doesn't exist", parent.display())))?;
    Ok(parent.join(name))
}

/// Resolves a path a pack named and checks the pack may touch it. A relative path is inside
/// the pack's data folder.
pub fn resolve(ctx: &Ctx, path: &str) -> Result<PathBuf> {
    if path.is_empty() {
        return Err(PackError::new("E0301", "path is empty"));
    }
    let named = expand(path);
    let named = if named.is_absolute() { named } else { ctx.data_dir.join(named) };
    let real = real(&named)?;
    if ctx.user {
        return Ok(real);
    }
    let text = real.display().to_string();
    let data = ctx.data_dir.canonicalize().unwrap_or_else(|_| ctx.data_dir.clone());
    let allowed = real.starts_with(&data)
        || ctx.picked.iter().any(|p| real == *p || real.starts_with(p))
        || ctx.perms.files.iter().any(|g| glob(&expand(g).display().to_string(), &text));
    if allowed {
        return Ok(real);
    }
    Err(PackError::new("E0403", format!("{} may not touch {text}: it has its data folder, {}; declare a glob in pack.toml (files = [\"~/Projects/**\"]) or ask the user with files.pick",
        ctx.pack, if ctx.perms.files.is_empty() { "and no declared globs".to_string() } else { format!("and {}", ctx.perms.files.join(", ")) })).learn("echospot dev docs files"))
}

pub fn register(out: &mut Vec<Spec>) {
    out.push(Spec::new("files.read", Kind::Read, Gate::Files, "Read a text file", "path:file", "text", |ctx, i| {
        Run::Now(resolve(ctx, text(i, "path")).and_then(|path| {
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_READ) {
                return Err(PackError::new("E0501", format!("{} is larger than {} MB", path.display(), MAX_READ >> 20)));
            }
            let bytes = std::fs::read(&path).map_err(|e| PackError::new("E0403", format!("can't read {}: {e}", path.display())))?;
            Done::value(Value::String(String::from_utf8_lossy(&bytes).to_string()))
        }))
    }));
    out.push(Spec::new("files.write", Kind::Effect, Gate::Files, "Write a text file, replacing it", "path:file, text:text", "bool", |ctx, i| {
        Run::Now(resolve(ctx, text(i, "path")).and_then(|path| {
            let before = std::fs::read_to_string(&path).ok();
            std::fs::write(&path, text(i, "text")).map_err(|e| PackError::new("E0403", format!("can't write {}: {e}", path.display())))?;
            let name = path.display().to_string();
            // Undo restores what the file held, or removes a file that wasn't there.
            let undo = match before {
                Some(before) => ("files.write".to_string(), json!({ "path": name, "text": before })),
                None => ("files.remove".to_string(), json!({ "path": name })),
            };
            Ok(Done { value: json!(true), undo: Some(undo), ..Default::default() })
        }))
    }));
    out.push(Spec::new("files.remove", Kind::Effect, Gate::Files, "Delete a file, keeping its text so the delete can be undone", "path:file", "bool", |ctx, i| {
        Run::Now(resolve(ctx, text(i, "path")).and_then(|path| {
            let before = std::fs::read_to_string(&path).map_err(|e| PackError::new("E0403", format!("can't read {} before deleting it: {e}", path.display())))?;
            std::fs::remove_file(&path).map_err(|e| PackError::new("E0403", format!("can't delete {}: {e}", path.display())))?;
            Ok(Done { value: json!(true), undo: Some(("files.write".into(), json!({ "path": path.display().to_string(), "text": before }))), ..Default::default() })
        }))
    }));
    out.push(Spec::new("files.list", Kind::Read, Gate::Files, "The files in a folder", "path:folder", "list<file>", |ctx, i| {
        Run::Now(resolve(ctx, text(i, "path")).and_then(|path| {
            let mut names: Vec<String> = std::fs::read_dir(&path).map_err(|e| PackError::new("E0403", format!("can't list {}: {e}", path.display())))?
                .flatten().map(|e| e.path().display().to_string()).collect();
            names.sort();
            Done::value(json!(names))
        }))
    }));
    out.push(Spec::new("files.pick", Kind::Read, Gate::Free, "Ask the user to pick a file or a folder; what they pick is granted to the pack", "title:text?, folder:bool?", "file?", |_, _| {
        // The runtime answers this one itself, with EchoSpot's own picker. Reaching here means
        // there is no window to show it in.
        Run::err(PackError::new("E0403", "files.pick needs EchoSpot's window: a person picks, and the choice is the permission").learn("echospot dev docs files"))
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::manifest::Permissions;

    #[test]
    fn globs_stay_in_their_folder_unless_doubled() {
        assert!(glob("/home/u/Projects/**", "/home/u/Projects/a/b.rs"));
        assert!(glob("/home/u/Notes/*.md", "/home/u/Notes/today.md"));
        assert!(!glob("/home/u/Notes/*.md", "/home/u/Notes/sub/today.md"));
        assert!(glob("/home/u/**/*.md", "/home/u/a/b/c.md"));
        assert!(!glob("/home/u/Projects/**", "/home/u/.ssh/id_ed25519"));
        assert!(!glob("/home/u/Projects/**", "/home/u/Projects-secret/x"));
    }

    #[test]
    fn a_pack_has_its_data_folder_and_what_it_declared() {
        let root = std::env::temp_dir().join(format!("echospot-fs-{}", std::process::id()));
        let (data, shared, private) = (root.join("data"), root.join("shared"), root.join("private"));
        for dir in [&data, &shared, &private] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(private.join("key"), "secret").unwrap();
        std::os::unix::fs::symlink(private.join("key"), shared.join("link")).unwrap();
        let ctx = Ctx { pack: "p".into(), data_dir: data.clone(), perms: Permissions { files: vec![format!("{}/**", shared.display())], ..Default::default() }, ..Default::default() };
        assert!(resolve(&ctx, "cache.json").unwrap().starts_with(data.canonicalize().unwrap()));
        assert!(resolve(&ctx, &shared.join("a.txt").display().to_string()).is_ok());
        assert_eq!(resolve(&ctx, &private.join("key").display().to_string()).unwrap_err().code, "E0403");
        // A link inside a granted folder doesn't reach outside it.
        assert_eq!(resolve(&ctx, &shared.join("link").display().to_string()).unwrap_err().code, "E0403");
        assert_eq!(resolve(&ctx, &format!("{}/../private/key", shared.display())).unwrap_err().code, "E0403");
        let picked = Ctx { picked: vec![private.canonicalize().unwrap()], ..ctx.clone() };
        assert!(resolve(&picked, &private.join("key").display().to_string()).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }
}
