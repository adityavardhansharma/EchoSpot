//! System packs: EchoSpot's own features written on the pack API, shipped inside the binary as
//! Luau source. They are unpacked beside the host's state, loaded like any pack with their grants
//! implied, and `echospot fork <id>` copies one into the user's packs, where the copy takes over.

use std::path::{Path, PathBuf};

use super::error::{PackError, Result};

pub struct SystemPack {
    pub id: &'static str,
    pub files: &'static [(&'static str, &'static str)],
}

pub const PACKS: &[SystemPack] = &[SystemPack {
    id: "activity",
    files: &[
        ("pack.toml", include_str!("system/activity/pack.toml")),
        ("init.luau", include_str!("system/activity/init.luau")),
        ("icons/timeline.svg", include_str!("system/activity/icons/timeline.svg")),
        ("INTENT.md", include_str!("system/activity/INTENT.md")),
    ],
}];

/// Where system packs are unpacked. Only packs under here have their grants implied.
pub fn dir() -> PathBuf {
    super::state_dir().join("system")
}

fn write(root: &Path, files: &[(&str, &str)]) -> std::io::Result<()> {
    for (name, text) in files {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Left alone when it already says the same, so a start doesn't touch every file.
        if std::fs::read_to_string(&path).ok().as_deref() != Some(*text) {
            std::fs::write(&path, text)?;
        }
    }
    Ok(())
}

/// Writes every system pack out and returns their folders.
pub fn unpack() -> Vec<PathBuf> {
    PACKS.iter().filter_map(|pack| {
        let root = dir().join(pack.id);
        write(&root, pack.files).ok().map(|()| root)
    }).collect()
}

/// Copies a system pack's source to the user's packs. From then on the copy is the pack with
/// that id: it is the user's, so it asks for its grants like any other.
pub fn fork(id: &str) -> Result<PathBuf> {
    let Some(pack) = PACKS.iter().find(|p| p.id == id) else {
        return Err(PackError::new("E0300", format!("no system pack {id:?}")).nearest(id, PACKS.iter().map(|p| p.id)).learn("echospot fork"));
    };
    let dest = super::packs_dir().join(id);
    if dest.exists() {
        return Err(PackError::new("E0102", format!("{} already exists; remove it to fork again", dest.display())));
    }
    write(&dest, pack.files).map_err(|e| PackError::new("E0200", format!("couldn't write {}: {e}", dest.display())))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::manifest::Manifest;

    /// API-DESIGN.md §24, M3's exit: the checker passes on every system pack, in every scene the host makes up.
    #[test]
    fn every_system_pack_checks_clean() {
        crate::packs::tests::isolate();
        for pack in PACKS {
            let dir = std::env::temp_dir().join(format!("echospot-system-check-{}", std::process::id())).join(pack.id);
            let _ = std::fs::remove_dir_all(&dir);
            for (name, text) in pack.files {
                let path = dir.join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
            let mut rt = crate::packs::runtime::Runtime::ephemeral();
            let report = crate::packs::check::check(&mut rt, &dir);
            assert!(report.errors.is_empty(), "{}: {:#?}", pack.id, report.errors);
            for drawn in crate::packs::check::design(&mut rt, pack.id, |_, _| None) {
                assert!(drawn.error.is_none(), "{} {} in {}: {:?}", pack.id, drawn.target, drawn.scene, drawn.error);
                assert!(!crate::packs::design::failed(&drawn.findings), "{} {} in {}: {:#?}", pack.id, drawn.target, drawn.scene, drawn.findings);
            }
            let _ = std::fs::remove_dir_all(dir.parent().unwrap());
        }
    }

    #[test]
    fn every_system_pack_has_a_valid_manifest() {
        for pack in PACKS {
            let toml = pack.files.iter().find(|(name, _)| *name == "pack.toml").expect("a pack.toml").1;
            let manifest = Manifest::parse(toml).unwrap();
            assert_eq!(manifest.id, pack.id);
            assert!(pack.files.iter().any(|(name, _)| *name == "init.luau"));
        }
    }
}
