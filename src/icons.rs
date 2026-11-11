//! The glyph set: line icons on a 24-unit grid, drawn at SF Symbols' medium weight.
#![allow(dead_code)]

use rune::Icon;

macro_rules! icons {
    ($($name:ident = $body:literal;)*) => {
        $(pub static $name: Icon = Icon {
            name: concat!("es-", stringify!($name)),
            view: 24.0,
            body: concat!(
                r#"<g fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">"#,
                $body, "</g>"),
        };)*
        /// Every glyph by its lowercase name, for config files and extensions.
        pub fn by_name(name: &str) -> Option<&'static Icon> {
            $(if name.eq_ignore_ascii_case(stringify!($name)) { return Some(&$name); })*
            None
        }
    };
}

icons! {
    SEARCH = r#"<circle cx="10.5" cy="10.5" r="6.5"/><path d="M15.5 15.5L20.5 20.5"/>"#;
    CHEVRON_LEFT = r#"<path d="M14.5 5L7.5 12l7 7"/>"#;
    CHEVRON_RIGHT = r#"<path d="M9.5 5l7 7-7 7"/>"#;
    CHEVRON_DOWN = r#"<path d="M6 9.5l6 6 6-6"/>"#;
    RETURN = r#"<path d="M19 5v6a3 3 0 0 1-3 3H6"/><path d="M9.5 10L5.5 14l4 4"/>"#;
    TAB = r#"<path d="M4 12h12"/><path d="M12 8l4 4-4 4"/><path d="M20 7v10"/>"#;
    MENU = r#"<path d="M5 9.5h14"/><path d="M5 14.5h8"/>"#;
    GEAR = r#"<circle cx="12" cy="12" r="3"/><path d="M12 2.8v2.4M12 18.8v2.4M2.8 12h2.4M18.8 12h2.4M5.5 5.5l1.7 1.7M16.8 16.8l1.7 1.7M5.5 18.5l1.7-1.7M16.8 7.2l1.7-1.7"/>"#;
    AI = r#"<path fill="currentColor" stroke-width="1.2" d="M10 3c.5 4.500 2.500 6.500 7 7-4.500.500-6.500 2.500-7 7-.500-4.500-2.500-6.500-7-7 4.500-.500 6.500-2.500 7-7z"/><path fill="currentColor" stroke-width="1" d="M18 14.300c.250 2.100 1.100 2.950 3.200 3.200-2.100.250-2.950 1.100-3.200 3.200-.250-2.100-1.100-2.950-3.200-3.200 2.100-.250 2.950-1.100 3.200-3.200z"/>"#;
    LOGO = r#"<circle cx="12" cy="12" r="2.700" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="6.200" stroke-width="2"/><path d="M13.740 2.150A10 10 0 0 1 21.850 10.260M10.260 21.850A10 10 0 0 1 2.150 13.740" stroke-width="2" opacity=".6"/>"#;
    SPARKLES = r#""<path d="M10 4l1.6 4.4L16 10l-4.4 1.6L10 16l-1.6-4.4L4 10l4.4-1.6z"/><path d="M18 14.5l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z"/>"#;
    FILE = r#"<path d="M14 3H7.5A1.5 1.5 0 0 0 6 4.5v15A1.5 1.5 0 0 0 7.5 21h9a1.5 1.5 0 0 0 1.5-1.5V7z"/><path d="M14 3v4h4"/>"#;
    FILE_TEXT = r#"<path d="M14 3H7.5A1.5 1.5 0 0 0 6 4.5v15A1.5 1.5 0 0 0 7.5 21h9a1.5 1.5 0 0 0 1.5-1.5V7z"/><path d="M14 3v4h4"/><path d="M9 12.5h6M9 16h4.5"/>"#;
    FILE_SEARCH = r#"<path d="M18 11V7l-4-4H7.5A1.5 1.5 0 0 0 6 4.5v15A1.5 1.5 0 0 0 7.5 21H11"/><path d="M14 3v4h4"/><circle cx="16" cy="16.5" r="2.7"/><path d="M18 18.5l2 2"/>"#;
    BOOK = r#"<path d="M6.5 3h11A1.5 1.5 0 0 1 19 4.5V21H7a2 2 0 0 1-2-2V4.5A1.5 1.5 0 0 1 6.5 3z"/><path d="M5 18.5A2 2 0 0 1 7 17h12"/>"#;
    CLIPBOARD = r#"<rect x="5" y="5" width="14" height="16" rx="2"/><rect x="9" y="3" width="6" height="4" rx="1"/><path d="M9 12h6M9 16h4"/>"#;
    SMILE = r#"<circle cx="12" cy="12" r="9"/><path d="M8.5 14.5a4.5 4.5 0 0 0 7 0"/><path d="M9 9.5v.5M15 9.5v.5"/>"#;
    FOLDER = r#"<path d="M3.5 7A1.5 1.5 0 0 1 5 5.5h4l2 2.5h8A1.5 1.5 0 0 1 20.5 9.5v8A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5z"/>"#;
    TERMINAL = r#"<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M7.5 9.5l3 2.5-3 2.5M13 15h3.5"/>"#;
    LOCK = r#"<rect x="5" y="10.5" width="14" height="10" rx="2.5"/><path d="M8 10.5V8a4 4 0 0 1 8 0v2.5"/>"#;
    MOON = r#"<path d="M20 14.5A8.5 8.5 0 1 1 9.5 4a7 7 0 0 0 10.5 10.5z"/>"#;
    POWER = r#"<path d="M12 3v8"/><path d="M7 6.3a8 8 0 1 0 10 0"/>"#;
    RESTART = r#"<path d="M20 12a8 8 0 1 1-2.6-5.9"/><path d="M20 4v4.5h-4.5"/>"#;
    LOGOUT = r#"<path d="M10 4H6.5A1.5 1.5 0 0 0 5 5.5v13A1.5 1.5 0 0 0 6.5 20H10"/><path d="M15 8l4 4-4 4M19 12H9.5"/>"#;
    VOLUME = r#"<path d="M4 9.5h3l4.5-4v13L7 14.5H4z"/><path d="M15.5 9a4.2 4.2 0 0 1 0 6M18 6.5a8 8 0 0 1 0 11"/>"#;
    VOLUME_LOW = r#"<path d="M4 9.5h3l4.5-4v13L7 14.5H4z"/><path d="M15.5 9a4.2 4.2 0 0 1 0 6"/>"#;
    VOLUME_X = r#"<path d="M4 9.5h3l4.5-4v13L7 14.5H4z"/><path d="M16 9.5l5 5M21 9.5l-5 5"/>"#;
    SUN = r#"<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"/>"#;
    TRASH = r#"<path d="M4.5 6.5h15M9.5 6.5V4.5h5v2M6.5 6.5l1 13A1.5 1.5 0 0 0 9 21h6a1.5 1.5 0 0 0 1.5-1.5l1-13"/><path d="M10 10.5v6M14 10.5v6"/>"#;
    WIFI = r#"<path d="M3 9.5a13 13 0 0 1 18 0M6.3 13a8.3 8.3 0 0 1 11.4 0M9.5 16.3a3.8 3.8 0 0 1 5 0"/><path d="M12 19.5v.1"/>"#;
    BLUETOOTH = r#"<path d="M7 8l10 8-5 4.5V3.5L17 8 7 16"/>"#;
    WINDOW = r#"<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M3 9h18"/>"#;
    WINDOWS = r#"<rect x="7.5" y="3.5" width="13" height="11" rx="2"/><path d="M16.5 14.5v4a2 2 0 0 1-2 2h-9a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2h2"/>"#;
    COPY = r#"<rect x="8.5" y="8.5" width="12" height="12" rx="2.5"/><path d="M15.5 8.5V6A2.5 2.5 0 0 0 13 3.5H6A2.5 2.5 0 0 0 3.5 6v7A2.5 2.5 0 0 0 6 15.5h2.5"/>"#;
    LINK = r#"<path d="M10 14a4.2 4.2 0 0 0 6 0l3.5-3.5a4.2 4.2 0 0 0-6-6L12.3 5.7"/><path d="M14 10a4.2 4.2 0 0 0-6 0l-3.5 3.5a4.2 4.2 0 0 0 6 6l1.2-1.2"/>"#;
    GLOBE = r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>"#;
    CALCULATOR = r#"<rect x="5" y="3" width="14" height="18" rx="2.5"/><path d="M8.5 7.5h7M8.5 12h.1M12 12h.1M15.5 12h.1M8.5 16h.1M12 16h.1M15.5 16h.1"/>"#;
    ARROW_RIGHT = r#"<path d="M4.5 12h15M13.5 6l6 6-6 6"/>"#;
    ARROW_UP_RIGHT = r#"<path d="M7 17L17 7M8.5 7H17v8.5"/>"#;
    PLUS = r#"<path d="M12 5v14M5 12h14"/>"#;
    STAR = r#"<path d="M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1.1 5.9-5.3-2.9-5.3 2.9 1.1-5.9-4.3-4.1 5.9-.8z"/>"#;
    PIN = r#"<path d="M9 3.5h6l-1 6 3.5 3.5v1.5h-11V13L10 9.5z"/><path d="M12 14.5V21"/>"#;
    EYE = r#"<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"/><circle cx="12" cy="12" r="3"/>"#;
    X = r#"<path d="M6 6l12 12M18 6L6 18"/>"#;
    CHECK = r#"<path d="M5 12.5l4.5 4.5L19 7.5"/>"#;
    INFO = r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v5.5M12 7.8v.1"/>"#;
    ALERT = r#"<path d="M12 4l9 15.5H3z"/><path d="M12 10v4.5M12 17v.1"/>"#;
    PUZZLE = r#"<path d="M9.5 4.5h3a1.8 1.8 0 1 1 3.4 0H19v4a1.8 1.8 0 1 0 0 3.5v4.5h-4.3a1.8 1.8 0 1 0-3.5 0H7v-4.2a1.8 1.8 0 1 1 0-3.6V4.5z"/>"#;
    DOWNLOAD = r#"<path d="M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19.5h14"/>"#;
    TEXT = r#"<path d="M5 6.5h14M5 11h14M5 15.5h9"/>"#;
    IMAGE = r#"<rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><circle cx="9" cy="10" r="1.6"/><path d="M4.5 17.5l5-4.5 3.5 3 2.5-2 4.5 4"/>"#;
    GRID = r#"<rect x="4" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5"/>"#;
    LIST = r#"<path d="M9 7h11M9 12h11M9 17h11M4.5 7h.1M4.5 12h.1M4.5 17h.1"/>"#;
    KEYBOARD = r#"<rect x="2.5" y="6" width="19" height="12" rx="2.5"/><path d="M6.5 10h.1M10 10h.1M13.5 10h.1M17 10h.1M8 14h8"/>"#;
    MAXIMIZE = r#"<path d="M4 9V5.5A1.5 1.5 0 0 1 5.5 4H9M15 4h3.5A1.5 1.5 0 0 1 20 5.5V9M20 15v3.5a1.5 1.5 0 0 1-1.5 1.5H15M9 20H5.5A1.5 1.5 0 0 1 4 18.5V15"/>"#;
    HALF_LEFT = r#"<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M12 4.5v15"/><path d="M6 9v6M9 9v6" stroke-width="2.4"/>"#;
    HALF_RIGHT = r#"<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M12 4.5v15"/><path d="M15 9v6M18 9v6" stroke-width="2.4"/>"#;
    CENTER = r#"<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><rect x="8" y="8.5" width="8" height="7" rx="1"/>"#;
    FLOAT = r#"<rect x="3" y="3" width="13" height="11" rx="2"/><rect x="9" y="9.5" width="12" height="11" rx="2"/>"#;
    PENCIL = r#"<path d="M4 20l1-4.5L16.5 4a2.1 2.1 0 0 1 3 3L8 18.5z"/><path d="M14.5 6l3 3"/>"#;
    PALETTE = r#"<path d="M12 3a9 9 0 1 0 0 18c1.5 0 2-1 2-2s-.8-1.6-.8-2.6 1-1.9 2.3-1.9H18a3 3 0 0 0 3-3c0-4.7-4-8.5-9-8.5z"/><path d="M7.5 12.5h.1M9.5 8.5h.1M14.5 8h.1"/>"#;
    BELL = r#"<path d="M6 16.5V11a6 6 0 1 1 12 0v5.5l1.5 2h-15z"/><path d="M10 20.5a2.2 2.2 0 0 0 4 0"/>"#;
    CAMERA = r#"<path d="M4 8.5A1.5 1.5 0 0 1 5.5 7H8l1.5-2.5h5L16 7h2.5A1.5 1.5 0 0 1 20 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5z"/><circle cx="12" cy="13" r="3.2"/>"#;
    CLOCK = r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v5.2l3.3 2"/>"#;
    HASH = r#"<path d="M9 4L7 20M17 4l-2 16M4.5 9h16M3.5 15h16"/>"#;
    BOLT = r#"<path d="M13 3L5 13.5h6L10 21l8-10.5h-6z"/>"#;
    DISPLAY = r#"<rect x="3" y="4" width="18" height="12.5" rx="2.5"/><path d="M9 20.5h6M12 16.5v4"/>"#;
    BATTERY = r#"<rect x="3" y="8" width="16" height="8.5" rx="2.5"/><path d="M21.5 11v2.5"/><path d="M6.5 11v2.5" stroke-width="2.6"/>"#;
    PERSON = r#"<circle cx="12" cy="8.5" r="3.8"/><path d="M5 20a7 7 0 0 1 14 0"/>"#;
    TAG = r#"<path d="M3.5 12.5V5A1.5 1.5 0 0 1 5 3.5h7.5l8 8-8.5 8.5z"/><path d="M8 8h.1"/>"#;
    CODE = r#"<path d="M8.5 7.5L4 12l4.5 4.5M15.5 7.5L20 12l-4.5 4.5"/>"#;
    HEART = r#"<path d="M12 20S4 15 4 9.5A4.3 4.3 0 0 1 12 7a4.3 4.3 0 0 1 8 2.5C20 15 12 20 12 20z"/>"#;
    MUSIC = r#"<path d="M9 18V6l10-2v12"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/>"#;
    PLAY = r#"<path d="M7.5 5l11 7-11 7z"/>"#;
    PAUSE = r#"<path d="M8 5v14M16 5v14"/>"#;
    SKIP = r#"<path d="M5.5 5l10 7-10 7zM18.5 5v14"/>"#;
    NIGHT = r#"<path d="M12 3v2M5.5 5.5l1.4 1.4M3 12h2M12 8a4 4 0 0 0 0 8z"/><path d="M12 8a4 4 0 0 1 0 8M12 19v2"/>"#;
    BOX = r#"<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/>"#;
    REFRESH = r#"<path d="M4 12a8 8 0 0 1 13.7-5.6L20 8.5M20 4v4.5h-4.5M20 12a8 8 0 0 1-13.7 5.6L4 15.500M4 20v-4.5h4.5"/>"#;
    HOME = r#"<path d="M4 11l8-7 8 7v8a1.500 1.5 0 0 1-1.5 1.5h-4v-6h-5v6h-4A1.500 1.5 0 0 1 4 19z"/>"#;
    CIRCLE = r#"<circle cx="12" cy="12" r="8.5"/>"#;
    DOT = r#"<circle cx="12" cy="12" r="3" fill="currentColor"/>"#;
    STORE = r#"<path d="M4 9l1.500-5h13L20 9M4 9v10.500h16V9M4 9a2.700 2.700 0 0 0 5.300 0 2.700 2.700 0 0 0 5.400 0A2.700 2.700 0 0 0 20 9"/>"#;
    UNDO = r#"<path d="M8 5L4 9l4 4M4 9h10a5.500 5.500 0 0 1 0 11h-3"/>"#;
    ELLIPSIS = r#"<path d="M6 12h.1M12 12h.1M18 12h.1" stroke-width="2.6"/>"#;
    SHARE = r#"<path d="M12 15V4M8 7.500L12 3.500l4 4M6 11.500v7A1.500 1.500 0 0 0 7.500 20h9a1.500 1.500 0 0 0 1.500-1.500v-7"/>"#;
    FILTER = r#"<path d="M4 6.500h16M7 12h10M10 17.500h4"/>"#;
    SIDEBAR = r#"<rect x="3" y="4.500" width="18" height="15" rx="2.500"/><path d="M14.500 4.500v15"/>"#;
    DICE = r#"<rect x="4" y="4" width="16" height="16" rx="3"/><path d="M8.500 8.500h.1M15.500 15.500h.1M12 12h.1M15.500 8.500h.1M8.500 15.500h.1" stroke-width="2.4"/>"#;
    KEY = r#"<circle cx="8" cy="15" r="4"/><path d="M11 12l8.500-8.500M16 7l2.500 2.500"/>"#;
    UPLOAD = r#"<path d="M12 15.500V4.500M7.500 9L12 4.500 16.500 9M5 19.500h14"/>"#;
    MIC = r#"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.500 11.500a6.500 6.500 0 0 0 13 0M12 18v3"/>"#;
    BRIGHTNESS_LOW = r#"<circle cx="12" cy="12" r="3.500"/><path d="M12 5v.1M12 19v.1M5 12h.1M19 12h.1M7 7l.1.1M17 17l.1.1M7 17l.1-.1M17 7l.1-.1"/>"#;
}
