//! Trust labels: facts about a pack, stated by the host. The broker sees every contact a pack
//! has with the world, so nothing here is the author's word.

use super::audit;
use super::index::Index;
use super::manifest::Permissions;

/// What can be said for certain about a pack with these permissions and this history.
pub fn labels(pack: &str, perms: &Permissions, index: &Index, entries: &[audit::Entry]) -> Vec<String> {
    let mut out = Vec::new();
    let mine: Vec<&audit::Entry> = entries.iter().filter(|e| e.pack == pack).collect();
    if perms.net.is_empty() && perms.ai.is_none() {
        out.push("Works offline".to_string());
    } else if !perms.net.is_empty() {
        out.push(format!("Talks only to {}", perms.net.join(", ")));
    }
    if perms.clipboard != "read" {
        out.push("Never reads your clipboard".to_string());
    }
    if perms.files.is_empty() {
        out.push("Touches no files but its own".to_string());
    }
    if perms.run.is_empty() {
        out.push("Runs no programs".to_string());
    }
    if !index.runs_in_background() {
        out.push("Runs only when you ask".to_string());
    }
    if perms.ai.is_some() {
        let since = audit::month_start();
        let spent: f64 = mine.iter().filter(|e| e.t >= since).map(|e| e.usd).sum();
        out.push(format!("Spent ${spent:.2} on AI this month"));
    } else {
        out.push("Uses no AI".to_string());
    }
    // What it has actually reached, as against what it may.
    let mut hosts: Vec<&str> = mine.iter().filter(|e| !e.host.is_empty() && e.host != "ai").map(|e| e.host.as_str()).collect();
    hosts.sort_unstable();
    hosts.dedup();
    if !hosts.is_empty() && hosts.len() < perms.net.len() {
        out.push(format!("Has only ever reached {}", hosts.join(", ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_facts() {
        let none = labels("p", &Permissions::default(), &Index::default(), &[]);
        assert!(none.contains(&"Works offline".to_string()) && none.contains(&"Never reads your clipboard".to_string()) && none.contains(&"Uses no AI".to_string()));
        let perms = Permissions { net: vec!["api.github.com".into(), "uploads.github.com".into()], clipboard: "read".into(), ..Default::default() };
        let entries = vec![audit::Entry { pack: "p".into(), host: "api.github.com".into(), ..Default::default() }];
        let some = labels("p", &perms, &Index::default(), &entries);
        assert!(some.contains(&"Talks only to api.github.com, uploads.github.com".to_string()));
        assert!(some.contains(&"Has only ever reached api.github.com".to_string()));
        assert!(!some.iter().any(|l| l.contains("clipboard")));
    }
}
