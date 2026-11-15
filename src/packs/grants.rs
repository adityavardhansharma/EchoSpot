//! Grants: what a person allowed a pack to do. They are given in EchoSpot's own window, by a
//! click or a key there, and nowhere else: no command, socket message or pack call writes them.
//!
//! The file is the host's, mode 0600. A pack's VM has no file access at all, so a pack can't
//! reach it; another program running as the user could, as it could reach anything of theirs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::manifest::{Ask, Permissions};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Grants {
    #[serde(default)]
    packs: BTreeMap<String, BTreeSet<Ask>>,
    /// Final abilities the user chose to stop being asked about, by address.
    #[serde(default)]
    pub no_confirm: BTreeSet<String>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

/// What a new version of a pack asks for, against what it has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diff {
    pub held: Vec<Ask>,
    pub new: Vec<Ask>,
    /// Granted before and no longer asked for.
    pub dropped: Vec<Ask>,
}

impl Grants {
    pub fn path() -> PathBuf {
        super::state_dir().join("grants.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        let mut g: Grants = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        g.path = Some(path);
        g
    }

    /// Grants that live only in memory: a test, a dry run.
    pub fn in_memory() -> Self {
        Self::default()
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let Ok(text) = serde_json::to_string_pretty(self) else { return };
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, text).is_ok() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            let _ = std::fs::rename(&tmp, path);
        }
    }

    pub fn has(&self, pack: &str, ask: &Ask) -> bool {
        self.packs.get(pack).is_some_and(|set| set.contains(ask))
    }

    pub fn held(&self, pack: &str) -> Vec<Ask> {
        self.packs.get(pack).map(|s| s.iter().cloned().collect()).unwrap_or_default()
    }

    /// What `perms` asks for that hasn't been granted.
    pub fn missing(&self, pack: &str, perms: &Permissions) -> Vec<Ask> {
        perms.asks().into_iter().filter(|a| !self.has(pack, a)).collect()
    }

    pub fn diff(&self, pack: &str, perms: &Permissions) -> Diff {
        let asks = perms.asks();
        let held = self.held(pack);
        Diff {
            new: asks.iter().filter(|a| !held.contains(a)).cloned().collect(),
            dropped: held.iter().filter(|a| !asks.contains(a)).cloned().collect(),
            held: asks.into_iter().filter(|a| held.contains(a)).collect(),
        }
    }

    /// Records a person's yes. Only the grants screen calls this.
    pub fn grant(&mut self, pack: &str, asks: impl IntoIterator<Item = Ask>) {
        self.packs.entry(pack.to_string()).or_default().extend(asks);
        self.save();
    }

    pub fn revoke(&mut self, pack: &str, ask: &Ask) {
        if let Some(set) = self.packs.get_mut(pack) {
            set.remove(ask);
        }
        self.save();
    }

    pub fn revoke_all(&mut self, pack: &str) {
        self.packs.remove(pack);
        self.save();
    }

    pub fn set_confirm(&mut self, address: &str, ask: bool) {
        if ask { self.no_confirm.remove(address); } else { self.no_confirm.insert(address.to_string()); }
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_update_that_asks_for_more_shows_the_difference() {
        let mut g = Grants::in_memory();
        let mut perms = Permissions { net: vec!["api.github.com".into()], ..Default::default() };
        assert_eq!(g.missing("github", &perms).len(), 1);
        g.grant("github", perms.asks());
        assert!(g.missing("github", &perms).is_empty());
        perms.net.push("uploads.github.com".into());
        perms.clipboard = "read".into();
        let diff = g.diff("github", &perms);
        assert_eq!((diff.held.len(), diff.new.len(), diff.dropped.len()), (1, 2, 0));
        // Write doesn't stand in for read: each line is granted as it was shown.
        assert!(!g.has("github", &Ask::new("clipboard", "read")));
        g.revoke("github", &Ask::new("net", "api.github.com"));
        assert_eq!(g.missing("github", &perms).len(), 3);
    }
}
