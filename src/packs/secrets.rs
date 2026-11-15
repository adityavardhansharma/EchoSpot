//! Secrets: API tokens the host holds for packs. A pack names one (`auth = "github"`) and the
//! host adds the header; the value never enters a VM, a log line or an argument list, and the
//! host refuses to send it anywhere but the hosts it is bound to.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Secret {
    pub value: String,
    /// The hosts it may be sent to, set by the person who stored it.
    pub hosts: Vec<String>,
}

fn path() -> PathBuf {
    std::env::var_os("ECHOSPOT_PACKS_STATE").map(PathBuf::from).unwrap_or_else(crate::util::config_dir).join("pack-secrets.json")
}

fn load() -> BTreeMap<String, Secret> {
    std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(all: &BTreeMap<String, Secret>) {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let Ok(text) = serde_json::to_string_pretty(all) else { return };
    let path = path();
    let tmp = path.with_extension("tmp");
    // Made private before a byte of it is written.
    let written = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()));
    if written.is_ok() {
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Names of the secrets that are set, and the hosts each is bound to. Never the values.
pub fn names() -> Vec<(String, Vec<String>)> {
    load().into_iter().map(|(name, s)| (name, s.hosts)).collect()
}

pub fn is_set(name: &str) -> bool {
    load().contains_key(name)
}

pub fn set(name: &str, value: &str, hosts: &[String]) {
    let mut all = load();
    all.insert(name.to_string(), Secret { value: value.trim().to_string(), hosts: hosts.iter().map(|h| h.to_lowercase()).collect() });
    save(&all);
}

pub fn remove(name: &str) {
    let mut all = load();
    all.remove(name);
    save(&all);
}

/// The header that carries secret `name` to `host`, or why it won't be sent.
pub fn header(name: &str, host: &str) -> Result<String, String> {
    let all = load();
    let Some(secret) = all.get(name) else { return Err(format!("the secret {name:?} isn't set yet: open EchoSpot, Packs, Set Secret")) };
    if !secret.hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
        return Err(format!("the secret {name:?} is bound to {}, not {host}", if secret.hosts.is_empty() { "no host".into() } else { secret.hosts.join(", ") }));
    }
    // `Header-Name: value` is sent as written; a bare token is a bearer token.
    let custom = secret.value.split_once(": ").is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    Ok(if custom { secret.value.clone() } else { format!("Authorization: Bearer {}", secret.value) })
}
