//! `~/.config/echospot/settings.json`: the few things a person tunes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::util;

/// Where Search Files looks. With nothing set it is the home folder and every mounted drive.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
#[serde(default)]
pub struct FileSearch {
    /// Folders left out of the search, with everything inside them.
    pub excluded: Vec<String>,
    /// Mounted drives that are not searched, by where they are mounted.
    pub drives_off: Vec<String>,
    /// Find hidden files and folders too.
    pub hidden: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    /// "system", "dark" or "light": the ink. Liquid Glass is a Surface, in either ink.
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
    /// translucent for Hyprland's own blur; "liquid" is Liquid Glass that bends the screen
    /// behind it; "none" draws it nearly opaque.
    pub backdrop: String,
    /// The currency a bare amount converts to in the calculator, as a code ("usd"). Empty
    /// follows the system's time zone and locale.
    pub currency: String,
    pub ai: crate::features::ai::AiSettings,
    /// Replace a snippet's keyword with its text as it is typed in any app.
    pub snippet_expansion: bool,
    /// Calendars to read for My Schedule: iCal addresses or `.ics` files.
    pub calendars: Vec<String>,
    /// The Liquid theme's glass.
    pub liquid: crate::glass::Level,
    pub files: FileSearch,
    /// The Look a pack is drawn under when it names none: "glass", "ink" or "ember".
    pub pack_look: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { appearance: "dark".into(), hide_on_blur: true, clipboard_history: true, top_margin: 0.18, animations: true, backdrop: "capture".into(), currency: String::new(), ai: Default::default(), calendars: Vec::new(), snippet_expansion: false, liquid: Default::default(), files: Default::default(), pack_look: "glass".into() }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        util::config_dir().join("settings.json")
    }
    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            util::write_json_private(&path, &Settings::default());
        }
        let mut settings: Settings = util::read_json(&path);
        settings.ai.normalize();
        // Liquid was a theme; it is a Surface now, in the system's ink as it was.
        if settings.appearance == "liquid" {
            settings.appearance = "system".into();
            settings.backdrop = "liquid".into();
        }
        settings
    }
    pub fn save(&self) {
        // The file can hold an API key: readable by this user only.
        util::write_json_private(&Self::path(), self);
    }
    /// The panel is a pane of Liquid Glass rather than one of the Surface setting's materials.
    pub fn liquid(&self) -> bool {
        self.backdrop == "liquid"
    }
    pub fn dark(&self, system_dark: bool) -> bool {
        match self.appearance.as_str() {
            "light" => false,
            "system" => system_dark,
            _ => true,
        }
    }
}
