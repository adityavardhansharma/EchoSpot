//! The index cache: what each pack declared, keyed by a hash of its files. The launcher reads
//! this, so installed packs cost a summon nothing: no VM starts until something runs.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::index::Index;

#[derive(Serialize, Deserialize)]
struct Cached {
    stamp: u64,
    index: Index,
}

fn path(id: &str) -> PathBuf {
    let dir = super::state_dir().join("index");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{id}.json"))
}

pub fn read(id: &str, stamp: u64) -> Option<Index> {
    let cached: Cached = serde_json::from_str(&std::fs::read_to_string(path(id)).ok()?).ok()?;
    (cached.stamp == stamp).then_some(cached.index)
}

pub fn write(id: &str, stamp: u64, index: &Index) {
    if let Ok(text) = serde_json::to_string(&Cached { stamp, index: index.clone() }) {
        let _ = std::fs::write(path(id), text);
    }
}
