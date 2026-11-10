//! `~/.config/echospot/settings.json`: the few things a person tunes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::util;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    /// "system", "dark" or "light".
    pub appearance: String,
    /// Hide the palette when it loses focus.
    pub hide_on_blur: bool,
    /// Keep a clipboard history.
    pub clipboard_history: bool,
    /// Fraction of the screen's height above the palette.
    pub top_margin: f32,
    pub animations: bool,
    /// What the panel's surface is made of: "capture" blurs a capture of the screen behind it
    /// (works with Hyprland's blur off, Omarchy's default); "compositor" leaves the panel
    /// translucent for Hyprland's own blur; "none" draws it nearly opaque.
    pub backdrop: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { appearance: "dark".into(), hide_on_blur: true, clipboard_history: true, top_margin: 0.18, animations: true, backdrop: "capture".into() }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        util::config_dir().join("settings.json")
    }
    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            util::write_json(&path, &Settings::default());
        }
        util::read_json(&path)
    }
    pub fn dark(&self, system_dark: bool) -> bool {
        match self.appearance.as_str() {
            "light" => false,
            "system" => system_dark,
            _ => true,
        }
    }
}
