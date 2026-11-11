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

pub fn dispatch(args: &[&str]) -> bool {
    let mut all = vec!["dispatch"];
    all.extend_from_slice(args);
    output("hyprctl", &all).is_some()
}

pub fn focus(address: &str) {
    dispatch(&["focuswindow", &format!("address:{address}")]);
}

pub fn close(address: &str) {
    dispatch(&["closewindow", &format!("address:{address}")]);
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

/// Places a window at a fraction of the work area. A tiled window floats first: Hyprland's
/// layouts own a tiled window's frame, so an exact frame is only a floating window's to hold.
fn place(target: &str, fx: f32, fy: f32, fw: f32, fh: f32) {
    let Some([x, y, w, h]) = work_area() else { return };
    let gap = 8.0;
    let (px, py) = (x + w * fx + gap, y + h * fy + gap);
    let (pw, ph) = (w * fw - gap * 2.0, h * fh - gap * 2.0);
    dispatch(&["setfloating", target]);
    dispatch(&["resizewindowpixel", &format!("exact {} {},{target}", pw.round(), ph.round())]);
    dispatch(&["movewindowpixel", &format!("exact {} {},{target}", px.round(), py.round())]);
}

/// Runs a layout command on `address`, the window that was focused before the palette opened.
pub fn run(op: WindowOp, address: &str) {
    let target = format!("address:{address}");
    let t = target.as_str();
    focus(address);
    match op {
        LeftHalf => place(t, 0.0, 0.0, 0.5, 1.0),
        RightHalf => place(t, 0.5, 0.0, 0.5, 1.0),
        TopHalf => place(t, 0.0, 0.0, 1.0, 0.5),
        BottomHalf => place(t, 0.0, 0.5, 1.0, 0.5),
        TopLeft => place(t, 0.0, 0.0, 0.5, 0.5),
        TopRight => place(t, 0.5, 0.0, 0.5, 0.5),
        BottomLeft => place(t, 0.0, 0.5, 0.5, 0.5),
        BottomRight => place(t, 0.5, 0.5, 0.5, 0.5),
        LeftThird => place(t, 0.0, 0.0, 1.0 / 3.0, 1.0),
        CenterThird => place(t, 1.0 / 3.0, 0.0, 1.0 / 3.0, 1.0),
        RightThird => place(t, 2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0),
        LeftTwoThirds => place(t, 0.0, 0.0, 2.0 / 3.0, 1.0),
        RightTwoThirds => place(t, 1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0),
        Maximize => place(t, 0.0, 0.0, 1.0, 1.0),
        AlmostMaximize => place(t, 0.05, 0.05, 0.9, 0.9),
        Center => {
            dispatch(&["setfloating", t]);
            dispatch(&["centerwindow"]);
        }
        Fullscreen => {
            dispatch(&["fullscreen", "0"]);
        }
        ToggleFloat => {
            dispatch(&["togglefloating", t]);
        }
        TogglePin => {
            dispatch(&["setfloating", t]);
            dispatch(&["pin", t]);
        }
        Close => close(address),
        NextWorkspace => {
            dispatch(&["workspace", "r+1"]);
        }
        PreviousWorkspace => {
            dispatch(&["workspace", "r-1"]);
        }
        MoveToNextWorkspace => {
            dispatch(&["movetoworkspace", &format!("r+1,{t}")]);
        }
        MoveToPreviousWorkspace => {
            dispatch(&["movetoworkspace", &format!("r-1,{t}")]);
        }
        NextDisplay => {
            dispatch(&["movewindow", "mon:+1"]);
        }
    }
}
