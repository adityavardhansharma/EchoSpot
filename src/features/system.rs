//! System commands: power, audio, display, connectivity and Omarchy's own toggles.

use rune::Icon;

use crate::icons;
use crate::theme::Tint;
use crate::util::{has_program, sh_ok, sh_output};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SystemAction {
    Lock,
    Sleep,
    Restart,
    Shutdown,
    Logout,
    VolumeUp,
    VolumeDown,
    ToggleMute,
    ToggleMicMute,
    BrightnessUp,
    BrightnessDown,
    EmptyTrash,
    ToggleWifi,
    ToggleBluetooth,
    ToggleNightLight,
    ToggleIdle,
    ToggleNotifications,
    ToggleBar,
    Screensaver,
    ScreenshotRegion,
    ScreenshotFull,
    ScreenRecord,
    ColorPicker,
    ExtractText,
    NextBackground,
    PlayPause,
    NextTrack,
    PreviousTrack,
    UpdateSystem,
    ReloadHyprland,
    RestartShell,
}

pub struct SystemCommand {
    pub action: SystemAction,
    pub title: &'static str,
    pub keywords: &'static str,
    pub icon: &'static Icon,
    /// Asks before running.
    pub confirm: Option<(&'static str, &'static str)>,
}

use SystemAction::*;

pub static COMMANDS: &[SystemCommand] = &[
    SystemCommand { action: Lock, title: "Lock Screen", keywords: "lock secure", icon: &icons::LOCK, confirm: None },
    SystemCommand { action: Sleep, title: "Sleep", keywords: "suspend standby", icon: &icons::MOON, confirm: None },
    SystemCommand { action: Restart, title: "Restart", keywords: "reboot", icon: &icons::RESTART, confirm: Some(("Restart this computer?", "Restart")) },
    SystemCommand { action: Shutdown, title: "Shut Down", keywords: "power off poweroff", icon: &icons::POWER, confirm: Some(("Shut down this computer?", "Shut Down")) },
    SystemCommand { action: Logout, title: "Log Out", keywords: "logout exit session", icon: &icons::LOGOUT, confirm: Some(("Log out of this session?", "Log Out")) },
    SystemCommand { action: VolumeUp, title: "Turn Volume Up", keywords: "audio louder sound", icon: &icons::VOLUME, confirm: None },
    SystemCommand { action: VolumeDown, title: "Turn Volume Down", keywords: "audio quieter sound", icon: &icons::VOLUME_LOW, confirm: None },
    SystemCommand { action: ToggleMute, title: "Toggle Mute", keywords: "audio silence sound", icon: &icons::VOLUME_X, confirm: None },
    SystemCommand { action: ToggleMicMute, title: "Toggle Microphone Mute", keywords: "mic input", icon: &icons::MIC, confirm: None },
    SystemCommand { action: BrightnessUp, title: "Increase Brightness", keywords: "display screen brighter", icon: &icons::SUN, confirm: None },
    SystemCommand { action: BrightnessDown, title: "Decrease Brightness", keywords: "display screen dimmer", icon: &icons::BRIGHTNESS_LOW, confirm: None },
    SystemCommand { action: EmptyTrash, title: "Empty Trash", keywords: "bin delete rubbish", icon: &icons::TRASH, confirm: Some(("Empty the Trash? This can't be undone.", "Empty Trash")) },
    SystemCommand { action: ToggleWifi, title: "Toggle Wi-Fi", keywords: "wifi wireless network", icon: &icons::WIFI, confirm: None },
    SystemCommand { action: ToggleBluetooth, title: "Toggle Bluetooth", keywords: "bluetooth", icon: &icons::BLUETOOTH, confirm: None },
    SystemCommand { action: ToggleNightLight, title: "Toggle Night Light", keywords: "night shift warm hyprsunset", icon: &icons::NIGHT, confirm: None },
    SystemCommand { action: ToggleIdle, title: "Toggle Stay Awake", keywords: "idle caffeine awake sleep lock", icon: &icons::EYE, confirm: None },
    SystemCommand { action: ToggleNotifications, title: "Toggle Do Not Disturb", keywords: "notifications silence dnd focus", icon: &icons::BELL, confirm: None },
    SystemCommand { action: ToggleBar, title: "Toggle Top Bar", keywords: "bar panel status", icon: &icons::SIDEBAR, confirm: None },
    SystemCommand { action: Screensaver, title: "Start Screensaver", keywords: "screen saver", icon: &icons::DISPLAY, confirm: None },
    SystemCommand { action: ScreenshotRegion, title: "Capture Screenshot", keywords: "screenshot region area snip", icon: &icons::CAMERA, confirm: None },
    SystemCommand { action: ScreenshotFull, title: "Capture Entire Screen", keywords: "screenshot fullscreen", icon: &icons::CAMERA, confirm: None },
    SystemCommand { action: ScreenRecord, title: "Record Screen", keywords: "screen recording video capture", icon: &icons::PLAY, confirm: None },
    SystemCommand { action: ColorPicker, title: "Pick Color", keywords: "colour eyedropper hex hyprpicker", icon: &icons::PALETTE, confirm: None },
    SystemCommand { action: ExtractText, title: "Capture Text from Screen", keywords: "ocr text recognize", icon: &icons::TEXT, confirm: None },
    SystemCommand { action: NextBackground, title: "Next Wallpaper", keywords: "background wallpaper", icon: &icons::IMAGE, confirm: None },
    SystemCommand { action: PlayPause, title: "Play / Pause", keywords: "media music", icon: &icons::PLAY, confirm: None },
    SystemCommand { action: NextTrack, title: "Next Track", keywords: "media music skip", icon: &icons::SKIP, confirm: None },
    SystemCommand { action: PreviousTrack, title: "Previous Track", keywords: "media music back", icon: &icons::UNDO, confirm: None },
    SystemCommand { action: UpdateSystem, title: "Update System", keywords: "omarchy update upgrade packages", icon: &icons::DOWNLOAD, confirm: None },
    SystemCommand { action: ReloadHyprland, title: "Reload Hyprland", keywords: "hyprctl reload config", icon: &icons::REFRESH, confirm: None },
    SystemCommand { action: RestartShell, title: "Restart Omarchy Shell", keywords: "bar quickshell restart", icon: &icons::REFRESH, confirm: None },
];

/// The colour family a command's tile wears: what it acts on, not what it is called.
pub fn tint(action: SystemAction) -> Tint {
    match action {
        Restart | Shutdown | EmptyTrash => Tint::Red,
        Lock | Sleep | Logout | UpdateSystem | ReloadHyprland | RestartShell | ToggleBar => Tint::Graphite,
        VolumeUp | VolumeDown | ToggleMute | ToggleMicMute | PlayPause | NextTrack | PreviousTrack => Tint::Pink,
        BrightnessUp | BrightnessDown | ToggleNightLight => Tint::Yellow,
        ToggleWifi | ToggleBluetooth => Tint::Blue,
        ToggleIdle => Tint::Orange,
        ToggleNotifications => Tint::Indigo,
        Screensaver | NextBackground => Tint::Cyan,
        ScreenshotRegion | ScreenshotFull | ScreenRecord | ColorPicker | ExtractText => Tint::Teal,
    }
}

pub fn command(action: SystemAction) -> &'static SystemCommand {
    COMMANDS.iter().find(|c| c.action == action).expect("every action has a command")
}

/// Whether this machine can run the command at all.
pub fn available(action: SystemAction) -> bool {
    match action {
        BrightnessUp | BrightnessDown => has_program("brightnessctl"),
        ToggleWifi => has_program("nmcli"),
        ToggleBluetooth => has_program("bluetoothctl"),
        ToggleNightLight => has_program("omarchy-toggle-nightlight") || has_program("hyprsunset"),
        ToggleIdle => has_program("omarchy-toggle-idle"),
        ToggleNotifications => has_program("omarchy-toggle-notification-silencing"),
        ToggleBar => has_program("omarchy-toggle-bar"),
        Screensaver => has_program("omarchy-launch-screensaver"),
        ScreenshotRegion | ScreenshotFull => has_program("omarchy-capture-screenshot") || has_program("grim"),
        ScreenRecord => has_program("omarchy-capture-screenrecording"),
        ColorPicker => has_program("hyprpicker"),
        ExtractText => has_program("omarchy-capture-text"),
        NextBackground => has_program("omarchy-theme-bg-next"),
        PlayPause | NextTrack | PreviousTrack => has_program("playerctl"),
        UpdateSystem => has_program("omarchy"),
        RestartShell => has_program("omarchy"),
        _ => true,
    }
}

/// What the machine's switches say right now, for the rows that flip them. `None` where it
/// could not be read.
#[derive(Clone, Default, Debug)]
pub struct Status {
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
    pub muted: Option<bool>,
    pub mic_muted: Option<bool>,
    pub volume: Option<u32>,
}

/// Reads the switches. Blocking, a few tens of milliseconds: call from a worker.
pub fn status() -> Status {
    let sink = sh_output("wpctl get-volume @DEFAULT_AUDIO_SINK@ 2>/dev/null");
    let source = sh_output("wpctl get-volume @DEFAULT_AUDIO_SOURCE@ 2>/dev/null");
    Status {
        wifi: has_program("nmcli").then(|| sh_output("nmcli -t radio wifi 2>/dev/null")).flatten().map(|o| o.trim() == "enabled"),
        // bluetoothctl waits on its daemon; a machine without one must not hold this up.
        bluetooth: has_program("bluetoothctl").then(|| sh_output("timeout 1 bluetoothctl show 2>/dev/null | grep -m1 Powered")).flatten()
            .filter(|o| !o.is_empty()).map(|o| o.contains("yes")),
        muted: sink.as_ref().map(|o| o.contains("MUTED")),
        mic_muted: source.map(|o| o.contains("MUTED")),
        volume: sink.and_then(|o| o.split_whitespace().nth(1).and_then(|v| v.parse::<f32>().ok())).map(|v| (v * 100.0).round() as u32),
    }
}

/// The state a command's row shows at its trailing edge, and whether that state is "on".
pub fn state(action: SystemAction, status: &Status) -> Option<(String, bool)> {
    let on_off = |on: bool| (if on { "On" } else { "Off" }.to_string(), on);
    match action {
        ToggleWifi => status.wifi.map(on_off),
        ToggleBluetooth => status.bluetooth.map(on_off),
        ToggleMute => status.muted.map(|m| (if m { "Muted" } else { "Sound On" }.to_string(), !m)),
        ToggleMicMute => status.mic_muted.map(|m| (if m { "Muted" } else { "Live" }.to_string(), !m)),
        VolumeUp | VolumeDown => status.volume.map(|v| (format!("{v}%"), false)),
        _ => None,
    }
}

/// What a command leaves behind: whether the palette closes first, and what to say after.
pub struct Outcome {
    pub message: Option<String>,
    pub ok: bool,
}

fn say(message: impl Into<String>) -> Outcome {
    Outcome { message: Some(message.into()), ok: true }
}

fn quiet() -> Outcome {
    Outcome { message: None, ok: true }
}

/// Whether the palette must be gone before the command runs (a screenshot, a lock).
pub fn closes_first(action: SystemAction) -> bool {
    !matches!(action, VolumeUp | VolumeDown | ToggleMute | ToggleMicMute | BrightnessUp | BrightnessDown
        | EmptyTrash | ToggleWifi | ToggleBluetooth | ToggleNightLight | ToggleIdle | ToggleNotifications
        | PlayPause | NextTrack | PreviousTrack | NextBackground | ReloadHyprland)
}

fn first_available(scripts: &[(&str, &str)]) -> bool {
    scripts.iter().find(|(program, _)| has_program(program)).is_some_and(|(_, script)| crate::util::sh_detached(script))
}

fn volume_text() -> String {
    let out = sh_output("wpctl get-volume @DEFAULT_AUDIO_SINK@").unwrap_or_default();
    if out.contains("MUTED") {
        return "Muted".into();
    }
    let level: f32 = out.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    format!("Volume {:.0}%", level * 100.0)
}

pub fn run(action: SystemAction) -> Outcome {
    match action {
        Lock => {
            first_available(&[("omarchy-system-lock", "omarchy-system-lock"), ("hyprlock", "hyprlock"), ("loginctl", "loginctl lock-session")]);
            quiet()
        }
        Sleep => {
            crate::util::sh_detached("systemctl suspend");
            quiet()
        }
        Restart => {
            first_available(&[("omarchy-system-reboot", "omarchy-system-reboot"), ("systemctl", "systemctl reboot")]);
            quiet()
        }
        Shutdown => {
            first_available(&[("omarchy-system-shutdown", "omarchy-system-shutdown"), ("systemctl", "systemctl poweroff")]);
            quiet()
        }
        Logout => {
            first_available(&[("omarchy-system-logout", "omarchy-system-logout"), ("uwsm", "uwsm stop"), ("hyprctl", "hyprctl dispatch exit")]);
            quiet()
        }
        VolumeUp => {
            sh_ok("wpctl set-mute @DEFAULT_AUDIO_SINK@ 0; wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+");
            say(volume_text())
        }
        VolumeDown => {
            sh_ok("wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-");
            say(volume_text())
        }
        ToggleMute => {
            sh_ok("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle");
            let muted = volume_text() == "Muted";
            say(if muted { "Muted".to_string() } else { format!("Unmuted · {}", volume_text()) })
        }
        ToggleMicMute => {
            sh_ok("wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle");
            let muted = sh_output("wpctl get-volume @DEFAULT_AUDIO_SOURCE@").is_some_and(|o| o.contains("MUTED"));
            say(if muted { "Microphone Muted" } else { "Microphone On" })
        }
        BrightnessUp | BrightnessDown => {
            sh_ok(if action == BrightnessUp { "brightnessctl -q set 5%+" } else { "brightnessctl -q set 5%-" });
            let pct = sh_output("brightnessctl -m | cut -d, -f4").unwrap_or_default();
            say(format!("Brightness {pct}"))
        }
        EmptyTrash => {
            let trash = crate::util::home().join(".local/share/Trash");
            let empty = std::fs::read_dir(trash.join("files")).map(|mut d| d.next().is_none()).unwrap_or(true);
            if empty {
                return Outcome { message: Some("Trash Is Already Empty".into()), ok: false };
            }
            if has_program("gio") {
                sh_ok("gio trash --empty");
            } else {
                let _ = std::fs::remove_dir_all(trash.join("files"));
                let _ = std::fs::remove_dir_all(trash.join("info"));
            }
            say("Trash Emptied")
        }
        ToggleWifi => {
            let on = sh_output("nmcli radio wifi").is_some_and(|s| s == "enabled");
            sh_ok(if on { "nmcli radio wifi off" } else { "nmcli radio wifi on" });
            say(if on { "Wi-Fi Off" } else { "Wi-Fi On" })
        }
        ToggleBluetooth => {
            let on = sh_output("bluetoothctl show").is_some_and(|s| s.contains("Powered: yes"));
            sh_ok(if on { "bluetoothctl power off" } else { "bluetoothctl power on" });
            say(if on { "Bluetooth Off" } else { "Bluetooth On" })
        }
        ToggleNightLight => {
            sh_ok("omarchy-toggle-nightlight >/dev/null 2>&1 || true");
            say("Night Light Toggled")
        }
        ToggleIdle => {
            sh_ok("omarchy-toggle-idle >/dev/null 2>&1");
            say("Stay Awake Toggled")
        }
        ToggleNotifications => {
            sh_ok("omarchy-toggle-notification-silencing >/dev/null 2>&1");
            say("Do Not Disturb Toggled")
        }
        ToggleBar => {
            crate::util::sh_detached("omarchy-toggle-bar");
            quiet()
        }
        Screensaver => {
            crate::util::sh_detached("omarchy-launch-screensaver");
            quiet()
        }
        ScreenshotRegion => {
            first_available(&[("omarchy-capture-screenshot", "sleep 0.25; omarchy-capture-screenshot"), ("grim", "sleep 0.25; grim -g \"$(slurp)\" - | wl-copy")]);
            quiet()
        }
        ScreenshotFull => {
            first_available(&[("omarchy-capture-screenshot", "sleep 0.25; omarchy-capture-screenshot fullscreen"), ("grim", "sleep 0.25; grim - | wl-copy")]);
            quiet()
        }
        ScreenRecord => {
            crate::util::sh_detached("sleep 0.25; omarchy-capture-screenrecording");
            quiet()
        }
        ColorPicker => {
            crate::util::sh_detached("sleep 0.25; hyprpicker -a");
            quiet()
        }
        ExtractText => {
            crate::util::sh_detached("sleep 0.25; omarchy-capture-text");
            quiet()
        }
        NextBackground => {
            crate::util::sh_detached("omarchy-theme-bg-next");
            say("Wallpaper Changed")
        }
        PlayPause => {
            sh_ok("playerctl play-pause");
            quiet()
        }
        NextTrack => {
            sh_ok("playerctl next");
            say("Next Track")
        }
        PreviousTrack => {
            sh_ok("playerctl previous");
            say("Previous Track")
        }
        UpdateSystem => {
            crate::util::sh_detached("omarchy launch floating terminal with presentation omarchy update || xdg-terminal-exec omarchy update");
            quiet()
        }
        ReloadHyprland => {
            let ok = sh_ok("hyprctl reload >/dev/null");
            Outcome { message: Some(if ok { "Hyprland Reloaded".into() } else { "Reload Failed".into() }), ok }
        }
        RestartShell => {
            crate::util::sh_detached("omarchy restart shell");
            quiet()
        }
    }
}
