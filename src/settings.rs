//! `~/.config/echospot/settings.json`: the few things a person tunes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::util;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    /// "system", "dark", "light" or "liquid": Liquid Glass, written in the system's ink.
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
}

impl Default for Settings {
    fn default() -> Self {
        Self { appearance: "dark".into(), hide_on_blur: true, clipboard_history: true, top_margin: 0.18, animations: true, backdrop: "capture".into(), currency: String::new(), ai: Default::default(), calendars: Vec::new(), snippet_expansion: false, liquid: Default::default() }
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
        let mut settings: Settings = util::read_json(&path);
        settings.ai.normalize();
        settings
    }
    pub fn save(&self) {
        let path = Self::path();
        util::write_json(&path, self);
        // The file can hold an API key: readable by this user only.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    /// The panel is a pane of Liquid Glass rather than one of the Surface setting's materials.
    pub fn liquid(&self) -> bool {
        self.appearance == "liquid"
    }
    pub fn dark(&self, system_dark: bool) -> bool {
        match self.appearance.as_str() {
            "light" => false,
            "system" | "liquid" => system_dark,
            _ => true,
        }
    }
}
