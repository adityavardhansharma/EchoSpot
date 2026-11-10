//! What the palette remembers between runs: how often each row is used, favorites, and the
//! icons it draws.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rune::{Image, ImageCache, Pixels};
use serde::{Deserialize, Serialize};

use crate::util;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Use {
    pub count: u32,
    pub last: u64,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Usage {
    #[serde(default)]
    pub uses: HashMap<String, Use>,
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub hidden: Vec<String>,
    #[serde(skip)]
    path: PathBuf,
}

impl Usage {
    pub fn load() -> Self {
        let path = util::data_dir().join("usage.json");
        let mut usage: Usage = util::read_json(&path);
        usage.path = path;
        usage
    }

    pub fn save(&self) {
        util::write_json(&self.path, self);
    }

    pub fn record(&mut self, id: &str) {
        let entry = self.uses.entry(id.to_string()).or_default();
        entry.count += 1;
        entry.last = util::now_secs();
        self.save();
    }

    /// Frequency weighted by recency: a row used today outranks one used often last month.
    pub fn frecency(&self, id: &str) -> f32 {
        let Some(u) = self.uses.get(id) else { return 0.0 };
        let days = util::now_secs().saturating_sub(u.last) as f32 / 86_400.0;
        let recency = if days < 0.2 { 4.0 } else if days < 1.0 { 3.0 } else if days < 4.0 { 2.0 } else if days < 14.0 { 1.0 } else if days < 60.0 { 0.5 } else { 0.25 };
        (u.count as f32).sqrt() * recency
    }

    pub fn is_favorite(&self, id: &str) -> bool {
        self.favorites.iter().any(|f| f == id)
    }

    pub fn toggle_favorite(&mut self, id: &str) -> bool {
        let now = if let Some(at) = self.favorites.iter().position(|f| f == id) {
            self.favorites.remove(at);
            false
        } else {
            self.favorites.push(id.to_string());
            true
        };
        self.save();
        now
    }
}

/// Icons by what they draw. A view asks with `&self`; decoding runs on Rune's image workers and
/// wakes the window when a picture is ready.
#[derive(Default)]
pub struct IconStore {
    cache: Option<ImageCache>,
    images: RefCell<HashMap<(PathBuf, u32, bool), Image>>,
}

impl IconStore {
    pub fn attach(&mut self, cache: ImageCache) {
        self.cache = Some(cache);
    }

    fn get(&self, path: &Path, size: f32, scale: f32, app: bool) -> Option<Pixels> {
        let cache = self.cache.as_ref()?;
        let key = (path.to_path_buf(), (size * scale).round() as u32, app);
        let mut images = self.images.borrow_mut();
        let image = images.entry(key).or_insert_with(|| {
            if app { cache.app_icon_scaled(path, size, scale) } else { cache.file(path, size, scale) }
        });
        if image.error().is_some() {
            return None;
        }
        Some(image.pixels().clone())
    }

    /// A `.desktop` entry's icon; `None` when it has none to draw.
    pub fn app(&self, desktop: &Path, size: f32, scale: f32) -> Option<Pixels> {
        self.get(desktop, size, scale, true)
    }

    pub fn file(&self, path: &Path, size: f32, scale: f32) -> Option<Pixels> {
        self.get(path, size, scale, false)
    }

    /// Whether the picture has decoded, so a view can hold its placeholder until then.
    pub fn ready(&self, path: &Path, size: f32, scale: f32, app: bool) -> bool {
        let key = (path.to_path_buf(), (size * scale).round() as u32, app);
        self.images.borrow().get(&key).is_some_and(|i| i.is_ready() && i.error().is_none())
    }

    /// Drops pictures of files that may have changed (a clipboard store pruned, a theme switched).
    pub fn forget(&self, under: &Path) {
        self.images.borrow_mut().retain(|(path, _, _), _| !path.starts_with(under));
    }
}
