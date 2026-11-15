//! Packs: personal software written by coding agents (API-DESIGN.md).
//!
//! A pack is a folder of Luau under `~/.config/echospot/packs/<id>`. It declares abilities (typed
//! functions with an address), surfaces, triggers and stores; every contact with the world goes
//! through one broker, which checks grants, writes the audit log and records how to undo.
#![allow(dead_code)]

use std::path::PathBuf;

use crate::util;

/// The API version this host speaks; it also runs packs written for the one before.
pub const API: u32 = 1;

/// Where packs live. `ECHOSPOT_PACKS_DIR` moves it, for tests and for `echospot dev`.
pub fn packs_dir() -> PathBuf {
    let dir = std::env::var_os("ECHOSPOT_PACKS_DIR").map(PathBuf::from).unwrap_or_else(|| util::config_dir().join("packs"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// What the host keeps about packs: grants, the audit log, stores, the index cache.
/// `ECHOSPOT_PACKS_STATE` moves it, so a test never touches the real one.
pub fn state_dir() -> PathBuf {
    let dir = std::env::var_os("ECHOSPOT_PACKS_STATE").map(PathBuf::from).unwrap_or_else(|| util::data_dir().join("packs"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// A pack's own data folder: always available to it, with no grant.
pub fn data_dir(pack: &str) -> PathBuf {
    let dir = state_dir().join("data").join(pack);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub mod error;
pub mod manifest;
pub mod types;
pub mod json;
pub mod grants;
pub mod audit;
pub mod index;
pub mod looks;
pub mod triggers;
pub mod stores;
pub mod builtins;
pub mod shared;
pub mod vm;
pub mod cache;
pub mod runtime;
#[cfg(test)]
pub mod tests;
pub mod secrets;
