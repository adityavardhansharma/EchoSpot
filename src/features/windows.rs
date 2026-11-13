//! Window management through Hyprland: the switcher's window list and the layout commands.

use rune::Icon;
use serde::Deserialize;

use crate::icons;
use crate::util::output;

#[derive(Clone, Debug, Deserialize, Default)]
pub struct Workspace {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct Client {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub workspace: Workspace,
    #[serde(default)]
    pub floating: bool,
    #[serde(default)]
    pub mapped: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub pid: i64,
    #[serde(default, rename = "focusHistoryID")]
    pub focus_history: i64,
    #[serde(default)]
    pub monitor: i64,
    #[serde(default)]
    pub at: [f32; 2],
    #[serde(default)]
    pub size: [f32; 2],
}

#[derive(Clone, Debug, Deserialize, Default)]
struct Monitor {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    width: f32,
    #[serde(default)]
    height: f32,
    #[serde(default)]
    scale: f32,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    reserved: [f32; 4],
    #[serde(default)]
    focused: bool,
    #[serde(default)]
    transform: i32,
    #[serde(default, rename = "activeWorkspace")]
    active_workspace: Workspace,
}

pub fn hyprland() -> bool {
    std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
}

/// Open windows, most recently focused first, without the palette's own.
pub fn clients() -> Vec<Client> {
    let Some(json) = output("hyprctl", &["clients", "-j"]) else { return Vec::new() };
    let mut clients: Vec<Client> = serde_json::from_str(&json).unwrap_or_default();
    clients.retain(|c| c.mapped && c.class != crate::APP_ID && !c.title.is_empty());
    clients.sort_by_key(|c| c.focus_history);
    clients
}

pub fn active() -> Option<Client> {
    let json = output("hyprctl", &["activewindow", "-j"])?;
    let client: Client = serde_json::from_str(&json).ok()?;
    (!client.address.is_empty() && client.class != crate::APP_ID).then_some(client)
}

/// Runs a dispatcher. Hyprland with a Lua config (Omarchy's) reads `hyprctl dispatch` as Lua, so
/// `lua` is tried first; one still on hyprlang rejects that and takes the `legacy` words.
/// The class of the window that has the keyboard, the palette's own included.
pub fn active_class() -> Option<String> {
    let json = output("hyprctl", &["activewindow", "-j"])?;
    serde_json::from_str::<Client>(&json).ok().map(|c| c.class)
}

pub fn dispatch(lua: &str, legacy: &[&str]) -> bool {
    if output("hyprctl", &["dispatch", lua]).is_some() {
        return true;
    }
    let mut all = vec!["dispatch"];
    all.extend_from_slice(legacy);
    output("hyprctl", &all).is_some()
}

/// A window as a dispatcher's Lua argument.
fn win(address: &str) -> String {
    format!("window = \"address:{address}\"")
}

pub fn focus(address: &str) {
    dispatch(&format!("hl.dsp.focus({{ {} }})", win(address)), &["focuswindow", &format!("address:{address}")]);
}

pub fn close(address: &str) {
    dispatch(&format!("hl.dsp.window.close({{ {} }})", win(address)), &["closewindow", &format!("address:{address}")]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowOp {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    LeftThird,
    CenterThird,
    RightThird,
    LeftTwoThirds,
    RightTwoThirds,
    Maximize,
    AlmostMaximize,
    Center,
    Fullscreen,
    ToggleFloat,
    TogglePin,
    Close,
    NextWorkspace,
    PreviousWorkspace,
    MoveToNextWorkspace,
    MoveToPreviousWorkspace,
    NextDisplay,
    PreviousDisplay,
    FirstFourth,
    SecondFourth,
    ThirdFourth,
    LastFourth,
    CenterHalf,
    CenterTwoThirds,
    ReasonableSize,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    Larger,
    Smaller,
    Restore,
    Tile,
}

pub struct WindowCommand {
    pub op: WindowOp,
    pub title: &'static str,
    pub keywords: &'static str,
    pub icon: &'static Icon,
}

use WindowOp::*;

pub static COMMANDS: &[WindowCommand] = &[
    WindowCommand { op: LeftHalf, title: "Left Half", keywords: "window tile snap", icon: &icons::HALF_LEFT },
    WindowCommand { op: RightHalf, title: "Right Half", keywords: "window tile snap", icon: &icons::HALF_RIGHT },
    WindowCommand { op: TopHalf, title: "Top Half", keywords: "window tile snap", icon: &icons::HALF_TOP },
    WindowCommand { op: BottomHalf, title: "Bottom Half", keywords: "window tile snap", icon: &icons::HALF_BOTTOM },
    WindowCommand { op: TopLeft, title: "Top Left Quarter", keywords: "window corner snap", icon: &icons::QUARTER_TL },
    WindowCommand { op: TopRight, title: "Top Right Quarter", keywords: "window corner snap", icon: &icons::QUARTER_TR },
    WindowCommand { op: BottomLeft, title: "Bottom Left Quarter", keywords: "window corner snap", icon: &icons::QUARTER_BL },
    WindowCommand { op: BottomRight, title: "Bottom Right Quarter", keywords: "window corner snap", icon: &icons::QUARTER_BR },
    WindowCommand { op: LeftThird, title: "Left Third", keywords: "window snap", icon: &icons::THIRD_LEFT },
    WindowCommand { op: CenterThird, title: "Center Third", keywords: "window snap", icon: &icons::THIRD_CENTER },
    WindowCommand { op: RightThird, title: "Right Third", keywords: "window snap", icon: &icons::THIRD_RIGHT },
    WindowCommand { op: LeftTwoThirds, title: "Left Two Thirds", keywords: "window snap", icon: &icons::TWO_THIRDS_LEFT },
    WindowCommand { op: RightTwoThirds, title: "Right Two Thirds", keywords: "window snap", icon: &icons::TWO_THIRDS_RIGHT },
    WindowCommand { op: Maximize, title: "Maximize Window", keywords: "window fill zoom", icon: &icons::MAXIMIZE },
    WindowCommand { op: AlmostMaximize, title: "Almost Maximize", keywords: "window fill", icon: &icons::ALMOST_MAXIMIZE },
    WindowCommand { op: Center, title: "Center Window", keywords: "window middle", icon: &icons::CENTER },
    WindowCommand { op: Fullscreen, title: "Toggle Fullscreen", keywords: "window full screen", icon: &icons::FULLSCREEN },
    WindowCommand { op: ToggleFloat, title: "Toggle Floating", keywords: "window float tile", icon: &icons::FLOAT },
    WindowCommand { op: TogglePin, title: "Toggle Pin on All Workspaces", keywords: "window sticky pin", icon: &icons::PIN },
    WindowCommand { op: Close, title: "Close Window", keywords: "window quit kill", icon: &icons::X },
    WindowCommand { op: NextWorkspace, title: "Next Workspace", keywords: "workspace desktop space", icon: &icons::ARROW_RIGHT },
    WindowCommand { op: PreviousWorkspace, title: "Previous Workspace", keywords: "workspace desktop space", icon: &icons::ARROW_LEFT },
    WindowCommand { op: MoveToNextWorkspace, title: "Move Window to Next Workspace", keywords: "workspace send", icon: &icons::WINDOW_ARROW_RIGHT },
    WindowCommand { op: MoveToPreviousWorkspace, title: "Move Window to Previous Workspace", keywords: "workspace send", icon: &icons::WINDOW_ARROW_LEFT },
    WindowCommand { op: NextDisplay, title: "Move Window to Next Display", keywords: "monitor screen send", icon: &icons::DISPLAY },
    WindowCommand { op: PreviousDisplay, title: "Move Window to Previous Display", keywords: "monitor screen send", icon: &icons::DISPLAY },
    WindowCommand { op: FirstFourth, title: "First Fourth", keywords: "window snap quarter column", icon: &icons::THIRD_LEFT },
    WindowCommand { op: SecondFourth, title: "Second Fourth", keywords: "window snap quarter column", icon: &icons::THIRD_CENTER },
    WindowCommand { op: ThirdFourth, title: "Third Fourth", keywords: "window snap quarter column", icon: &icons::THIRD_CENTER },
    WindowCommand { op: LastFourth, title: "Last Fourth", keywords: "window snap quarter column", icon: &icons::THIRD_RIGHT },
    WindowCommand { op: CenterHalf, title: "Center Half", keywords: "window snap middle", icon: &icons::THIRD_CENTER },
    WindowCommand { op: CenterTwoThirds, title: "Center Two Thirds", keywords: "window snap middle", icon: &icons::THIRD_CENTER },
    WindowCommand { op: ReasonableSize, title: "Reasonable Size", keywords: "window resize medium", icon: &icons::ALMOST_MAXIMIZE },
    WindowCommand { op: MoveLeft, title: "Move Window to Left Edge", keywords: "window nudge edge", icon: &icons::ARROW_LEFT },
    WindowCommand { op: MoveRight, title: "Move Window to Right Edge", keywords: "window nudge edge", icon: &icons::ARROW_RIGHT },
    WindowCommand { op: MoveUp, title: "Move Window to Top Edge", keywords: "window nudge edge", icon: &icons::ARROW_UP },
    WindowCommand { op: MoveDown, title: "Move Window to Bottom Edge", keywords: "window nudge edge", icon: &icons::DOWNLOAD },
    WindowCommand { op: Larger, title: "Make Window Larger", keywords: "window grow resize bigger", icon: &icons::MAXIMIZE },
    WindowCommand { op: Smaller, title: "Make Window Smaller", keywords: "window shrink resize", icon: &icons::CENTER },
    WindowCommand { op: Restore, title: "Restore Window", keywords: "window undo previous size position", icon: &icons::UNDO },
    WindowCommand { op: Tile, title: "Tile Window", keywords: "window stop floating layout", icon: &icons::GRID },
];

/// The usable area of the focused monitor in layout points: `[x, y, w, h]`.
fn work_area() -> Option<[f32; 4]> {
    let json = output("hyprctl", &["monitors", "-j"])?;
    let monitors: Vec<Monitor> = serde_json::from_str(&json).ok()?;
    let m = monitors.iter().find(|m| m.focused).or(monitors.first())?;
    let scale = if m.scale > 0.0 { m.scale } else { 1.0 };
    let (mut w, mut h) = (m.width / scale, m.height / scale);
    if m.transform % 2 == 1 {
        std::mem::swap(&mut w, &mut h);
    }
    let [left, top, right, bottom] = m.reserved;
    let _ = m.id;
    Some([m.x + left, m.y + top, w - left - right, h - top - bottom])
}

/// Where a window is right now.
fn find(address: &str) -> Option<Client> {
    let json = output("hyprctl", &["clients", "-j"])?;
    serde_json::from_str::<Vec<Client>>(&json).ok()?.into_iter().find(|c| c.address == address)
}

/// The frame each window had before the last command moved it: what Restore goes back to.
fn before() -> &'static std::sync::Mutex<std::collections::HashMap<String, (bool, [f32; 4])>> {
    static BEFORE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, (bool, [f32; 4])>>> = std::sync::OnceLock::new();
    BEFORE.get_or_init(Default::default)
}

fn remember(address: &str) {
    if let Some(c) = find(address)
        && let Ok(mut map) = before().lock() {
        map.insert(address.to_string(), (c.floating, [c.at[0], c.at[1], c.size[0], c.size[1]]));
    }
}

/// Gives a window an exact frame. A tiled window floats first: Hyprland's layouts own a tiled
/// window's frame, so an exact frame is only a floating window's to hold.
fn set_frame(address: &str, [px, py, pw, ph]: [f32; 4]) {
    let (px, py, pw, ph) = (px.round(), py.round(), pw.round().max(80.0), ph.round().max(60.0));
    let (w, target) = (win(address), format!("address:{address}"));
    // A fullscreen or maximized window ignores its frame until it is let out.
    for mode in ["fullscreen", "maximized"] {
        output("hyprctl", &["dispatch", &format!("hl.dsp.window.fullscreen({{ {w}, mode = \"{mode}\", action = \"unset\" }})")]);
    }
    float(address);
    dispatch(&format!("hl.dsp.window.resize({{ {w}, x = {pw}, y = {ph} }})"), &["resizewindowpixel", &format!("exact {pw} {ph},{target}")]);
    dispatch(&format!("hl.dsp.window.move({{ {w}, x = {px}, y = {py} }})"), &["movewindowpixel", &format!("exact {px} {py},{target}")]);
}

const GAP: f32 = 8.0;

/// Places a window at a fraction of the work area.
fn place(address: &str, fx: f32, fy: f32, fw: f32, fh: f32) {
    let Some([x, y, w, h]) = work_area() else { return };
    remember(address);
    set_frame(address, [x + w * fx + GAP, y + h * fy + GAP, w * fw - GAP * 2.0, h * fh - GAP * 2.0]);
}

/// The windows a snap layout arranges: those sharing a workspace with `first` (the window that
/// had focus), or on the focused monitor's workspace when nothing had it. `first` leads.
pub fn on_workspace(first: Option<&Client>) -> Vec<Client> {
    let workspace = match first {
        Some(c) => c.workspace.id,
        None => {
            let monitors: Vec<Monitor> = output("hyprctl", &["monitors", "-j"]).and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
            match monitors.iter().find(|m| m.focused).or(monitors.first()) {
                Some(m) => m.active_workspace.id,
                None => return Vec::new(),
            }
        }
    };
    let mut all = clients();
    all.retain(|c| !c.hidden && c.workspace.id == workspace);
    if let Some(at) = first.and_then(|f| all.iter().position(|c| c.address == f.address)) {
        let lead = all.remove(at);
        all.insert(0, lead);
    }
    all
}

/// Puts the palette back on top with the keyboard: a window floated into a zone while the palette
/// is open can land over it.
pub fn raise_palette() {
    let all: Vec<Client> = output("hyprctl", &["clients", "-j"]).and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    if let Some(me) = all.iter().find(|c| c.class == crate::APP_ID && c.title == "EchoSpot") {
        focus(&me.address);
    }
}

/// Places a window in a snap zone: `[x, y, w, h]` as fractions of the work area.
pub fn snap(address: &str, [fx, fy, fw, fh]: [f32; 4]) {
    place(address, fx, fy, fw, fh);
}

/// The frame of the `overflow`th window past a layout's last zone: stepped down and in, so each
/// one stacked there still shows.
fn cascaded([x, y, w, h]: [f32; 4], overflow: usize) -> [f32; 4] {
    let off = 28.0 * overflow as f32;
    [x + off, y + off, (w - off).max(200.0), (h - off).max(150.0)]
}

/// Gives every window a zone, in order. Windows past the last zone cascade inside it.
pub fn arrange(addresses: &[String], zones: &[[f32; 4]]) {
    let (Some([x, y, w, h]), Some(last)) = (work_area(), zones.len().checked_sub(1)) else { return };
    for (i, address) in addresses.iter().enumerate() {
        let [fx, fy, fw, fh] = zones[i.min(last)];
        let frame = [x + w * fx + GAP, y + h * fy + GAP, w * fw - GAP * 2.0, h * fh - GAP * 2.0];
        remember(address);
        set_frame(address, cascaded(frame, i.saturating_sub(last)));
    }
    if let Some(lead) = addresses.first() {
        focus(lead);
    }
}

/// Changes a window's frame from the one it has: a nudge to an edge, a step larger or smaller.
fn adjust(address: &str, change: impl Fn([f32; 4], [f32; 4]) -> [f32; 4]) {
    let Some(area) = work_area() else { return };
    remember(address);
    // A tiled window's frame is the layout's; float it and read the frame it floats at.
    float(address);
    let Some(c) = find(address) else { return };
    set_frame(address, change([c.at[0], c.at[1], c.size[0], c.size[1]], area));
}

/// Scales a frame about its centre, kept inside the work area.
fn scaled([x, y, w, h]: [f32; 4], [ax, ay, aw, ah]: [f32; 4], by: f32) -> [f32; 4] {
    let (nw, nh) = ((w * by).clamp(240.0, aw - GAP * 2.0), (h * by).clamp(160.0, ah - GAP * 2.0));
    let nx = (x + (w - nw) / 2.0).clamp(ax + GAP, ax + aw - GAP - nw);
    let ny = (y + (h - nh) / 2.0).clamp(ay + GAP, ay + ah - GAP - nh);
    [nx, ny, nw, nh]
}

fn float(address: &str) {
    dispatch(&format!("hl.dsp.window.float({{ {}, action = \"enable\" }})", win(address)), &["setfloating", &format!("address:{address}")]);
}

/// The workspace showing on the monitor `step` after the focused one, if there is another monitor.
fn display_workspace(step: usize) -> Option<i64> {
    let monitors: Vec<Monitor> = serde_json::from_str(&output("hyprctl", &["monitors", "-j"])?).ok()?;
    let at = monitors.iter().position(|m| m.focused)?;
    (monitors.len() > 1).then(|| monitors[(at + step) % monitors.len()].active_workspace.id)
}

fn to_display(address: &str, step: usize, legacy: &str) {
    // Hyprland's Lua has no "move to monitor": the window goes to the workspace that monitor shows.
    let step = if step == 0 { 1 } else { step };
    if let Some(id) = display_workspace(step) {
        dispatch(&format!("hl.dsp.window.move({{ {}, workspace = \"{id}\" }})", win(address)), &["movewindow", legacy]);
    }
}

/// Runs a layout command on `address`, the window that was focused before the palette opened.
pub fn run(op: WindowOp, address: &str) {
    let w = win(address);
    let target = format!("address:{address}");
    let t = target.as_str();
    focus(address);
    match op {
        LeftHalf => place(address, 0.0, 0.0, 0.5, 1.0),
        RightHalf => place(address, 0.5, 0.0, 0.5, 1.0),
        TopHalf => place(address, 0.0, 0.0, 1.0, 0.5),
        BottomHalf => place(address, 0.0, 0.5, 1.0, 0.5),
        TopLeft => place(address, 0.0, 0.0, 0.5, 0.5),
        TopRight => place(address, 0.5, 0.0, 0.5, 0.5),
        BottomLeft => place(address, 0.0, 0.5, 0.5, 0.5),
        BottomRight => place(address, 0.5, 0.5, 0.5, 0.5),
        LeftThird => place(address, 0.0, 0.0, 1.0 / 3.0, 1.0),
        CenterThird => place(address, 1.0 / 3.0, 0.0, 1.0 / 3.0, 1.0),
        RightThird => place(address, 2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0),
        LeftTwoThirds => place(address, 0.0, 0.0, 2.0 / 3.0, 1.0),
        RightTwoThirds => place(address, 1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0),
        Maximize => place(address, 0.0, 0.0, 1.0, 1.0),
        AlmostMaximize => place(address, 0.05, 0.05, 0.9, 0.9),
        Center => {
            float(address);
            dispatch(&format!("hl.dsp.window.center({{ {w} }})"), &["centerwindow"]);
        }
        Fullscreen => {
            dispatch(&format!("hl.dsp.window.fullscreen({{ {w}, mode = \"fullscreen\", action = \"toggle\" }})"), &["fullscreen", "0"]);
        }
        ToggleFloat => {
            dispatch(&format!("hl.dsp.window.float({{ {w}, action = \"toggle\" }})"), &["togglefloating", t]);
        }
        TogglePin => {
            float(address);
            dispatch(&format!("hl.dsp.window.pin({{ {w}, action = \"toggle\" }})"), &["pin", t]);
        }
        Close => close(address),
        NextWorkspace => {
            dispatch("hl.dsp.focus({ workspace = \"r+1\" })", &["workspace", "r+1"]);
        }
        PreviousWorkspace => {
            dispatch("hl.dsp.focus({ workspace = \"r-1\" })", &["workspace", "r-1"]);
        }
        MoveToNextWorkspace => {
            dispatch(&format!("hl.dsp.window.move({{ {w}, workspace = \"r+1\" }})"), &["movetoworkspace", &format!("r+1,{t}")]);
        }
        MoveToPreviousWorkspace => {
            dispatch(&format!("hl.dsp.window.move({{ {w}, workspace = \"r-1\" }})"), &["movetoworkspace", &format!("r-1,{t}")]);
        }
        NextDisplay => to_display(address, 1, "mon:+1"),
        PreviousDisplay => {
            let count = output("hyprctl", &["monitors", "-j"]).and_then(|j| serde_json::from_str::<Vec<Monitor>>(&j).ok()).map(|m| m.len()).unwrap_or(1);
            to_display(address, count.saturating_sub(1), "mon:-1");
        }
        FirstFourth => place(address, 0.0, 0.0, 0.25, 1.0),
        SecondFourth => place(address, 0.25, 0.0, 0.25, 1.0),
        ThirdFourth => place(address, 0.5, 0.0, 0.25, 1.0),
        LastFourth => place(address, 0.75, 0.0, 0.25, 1.0),
        CenterHalf => place(address, 0.25, 0.0, 0.5, 1.0),
        CenterTwoThirds => place(address, 1.0 / 6.0, 0.0, 2.0 / 3.0, 1.0),
        ReasonableSize => place(address, 0.2, 0.15, 0.6, 0.7),
        MoveLeft => adjust(address, |[_, y, w, h], [ax, ..]| [ax + GAP, y, w, h]),
        MoveRight => adjust(address, |[_, y, w, h], [ax, _, aw, _]| [ax + aw - GAP - w, y, w, h]),
        MoveUp => adjust(address, |[x, _, w, h], [_, ay, ..]| [x, ay + GAP, w, h]),
        MoveDown => adjust(address, |[x, _, w, h], [_, ay, _, ah]| [x, ay + ah - GAP - h, w, h]),
        Larger => adjust(address, |f, area| scaled(f, area, 1.15)),
        Smaller => adjust(address, |f, area| scaled(f, area, 1.0 / 1.15)),
        Restore => {
            let was = before().lock().ok().and_then(|mut m| m.remove(address));
            match was {
                Some((true, frame)) => set_frame(address, frame),
                Some((false, _)) => {
                    dispatch(&format!("hl.dsp.window.float({{ {w}, action = \"disable\" }})"), &["settiled", t]);
                }
                None => {}
            }
        }
        Tile => {
            dispatch(&format!("hl.dsp.window.float({{ {w}, action = \"disable\" }})"), &["settiled", t]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_past_the_last_zone_cascade() {
        let zone = [100.0, 50.0, 800.0, 600.0];
        assert_eq!(cascaded(zone, 0), zone);
        assert_eq!(cascaded(zone, 2), [156.0, 106.0, 744.0, 544.0]);
        // However many pile up, a window keeps a usable size.
        assert_eq!(cascaded(zone, 40)[2..], [200.0, 150.0]);
    }
}
