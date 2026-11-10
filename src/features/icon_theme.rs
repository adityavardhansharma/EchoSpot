//! Freedesktop icon lookup: the current theme, what it inherits, then hicolor and pixmaps.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::util;

fn base_dirs() -> Vec<PathBuf> {
    let home = util::home();
    let mut dirs = vec![home.join(".local/share/icons"), home.join(".icons")];
    let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').filter(|s| !s.is_empty()).map(|d| PathBuf::from(d).join("icons")));
    dirs.push(home.join(".local/share/flatpak/exports/share/icons"));
    dirs.push("/var/lib/flatpak/exports/share/icons".into());
    dirs
}

fn current_theme() -> String {
    let from_gtk = |file: &str| std::fs::read_to_string(util::home().join(file)).ok().and_then(|text| {
        text.lines().find_map(|l| l.strip_prefix("gtk-icon-theme-name=").map(|v| v.trim().trim_matches('"').to_string()))
    });
    util::output("gsettings", &["get", "org.gnome.desktop.interface", "icon-theme"])
        .map(|s| s.trim_matches('\'').to_string()).filter(|s| !s.is_empty())
        .or_else(|| from_gtk(".config/gtk-3.0/settings.ini"))
        .or_else(|| from_gtk(".config/gtk-4.0/settings.ini"))
        .unwrap_or_else(|| "hicolor".into())
}

/// The theme, everything it inherits (breadth first), and the fallbacks every lookup ends on.
fn chain(bases: &[PathBuf]) -> Vec<String> {
    let mut order = vec![current_theme()];
    let mut at = 0;
    while at < order.len() {
        for base in bases {
            let Ok(index) = std::fs::read_to_string(base.join(&order[at]).join("index.theme")) else { continue };
            if let Some(inherits) = index.lines().find_map(|l| l.strip_prefix("Inherits=")) {
                for parent in inherits.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    if !order.iter().any(|o| o == parent) {
                        order.push(parent.to_string());
                    }
                }
            }
            break;
        }
        at += 1;
    }
    for fallback in ["hicolor", "Adwaita", "breeze"] {
        if !order.iter().any(|o| o == fallback) {
            order.push(fallback.to_string());
        }
    }
    order
}

/// How good a directory's icons are for a launcher row: vectors, then the largest bitmaps.
fn dir_score(size_dir: &str) -> i32 {
    if size_dir.starts_with("scalable") {
        return 600;
    }
    let size: i32 = size_dir.split(['x', '@']).next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let hidpi = if size_dir.contains('@') { 2 } else { 1 };
    match size * hidpi {
        s if s >= 64 => 500 - (s - 128).abs() / 8,
        s => s,
    }
}

pub struct IconIndex {
    /// Name to file, per theme in lookup order.
    themes: Vec<HashMap<String, (i32, PathBuf)>>,
    loose: HashMap<String, PathBuf>,
}

impl IconIndex {
    /// Indexes application-relevant icon directories. Runs once at startup.
    pub fn build() -> Self {
        let bases = base_dirs();
        let mut themes = Vec::new();
        for theme in chain(&bases) {
            let mut map: HashMap<String, (i32, PathBuf)> = HashMap::new();
            for base in &bases {
                let Ok(sizes) = std::fs::read_dir(base.join(&theme)) else { continue };
                for size in sizes.flatten() {
                    let size_name = size.file_name().to_string_lossy().to_string();
                    let score = dir_score(&size_name);
                    // `48x48/apps/x.png` and the older `apps/48/x.png` layouts both occur.
                    let Ok(categories) = std::fs::read_dir(size.path()) else { continue };
                    for category in categories.flatten() {
                        let cat_name = category.file_name().to_string_lossy().to_string();
                        let score = if score == 0 { dir_score(&cat_name) } else { score };
                        let Ok(files) = std::fs::read_dir(category.path()) else { continue };
                        for file in files.flatten() {
                            let path = file.path();
                            let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };
                            if !matches!(ext, "png" | "svg" | "svgz") {
                                continue;
                            }
                            let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else { continue };
                            if map.get(&stem).is_none_or(|(best, _)| score > *best) {
                                map.insert(stem, (score, path));
                            }
                        }
                    }
                }
            }
            themes.push(map);
        }
        let mut loose = HashMap::new();
        let pixmaps = [PathBuf::from("/usr/share/pixmaps"), PathBuf::from("/usr/local/share/pixmaps")];
        for dir in bases.iter().chain(pixmaps.iter()) {
            let Ok(files) = std::fs::read_dir(dir) else { continue };
            for file in files.flatten() {
                let path = file.path();
                if path.is_file() && matches!(path.extension().and_then(|e| e.to_str()), Some("png" | "svg" | "xpm" | "svgz")) {
                    if let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) {
                        loose.entry(stem).or_insert(path);
                    }
                }
            }
        }
        Self { themes, loose }
    }

    /// The file for an `Icon=` value: a path as is, else a name through the theme chain.
    pub fn find(&self, icon: &str) -> Option<PathBuf> {
        if icon.is_empty() {
            return None;
        }
        if icon.starts_with('/') {
            return Path::new(icon).is_file().then(|| PathBuf::from(icon));
        }
        // Some entries name the file with its extension.
        let name = icon.strip_suffix(".png").or_else(|| icon.strip_suffix(".svg")).unwrap_or(icon);
        self.themes.iter().find_map(|t| t.get(name).map(|(_, p)| p.clone()))
            .or_else(|| self.loose.get(name).filter(|p| p.extension().is_none_or(|e| e != "xpm")).cloned())
    }
}
