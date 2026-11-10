//! Design tokens. Every value is Tinycast's own (`DesignSystem/Theme.swift`): the dark column is
//! the design, and nothing in a view states a number that has a token here.

use rune::{Color, Spring, TextStyle};

pub mod space {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 10.0;
    pub const XL: f32 = 12.0;
    pub const DIALOG_INSET: f32 = 18.0;
    pub const XXL: f32 = 20.0;
    /// Header → first row.
    pub const SECTION_HEADER_BOTTOM: f32 = 4.0;
    /// Above every header but the list's first.
    pub const SECTION_SPACING: f32 = 12.0;
    pub const EMOJI_SECTION_SPACING: f32 = 14.0;
}

pub mod radius {
    pub const PANEL: f32 = 26.0;
    pub const ROW: f32 = 10.0;
    pub const FORM_FIELD: f32 = 12.0;
    pub const EMOJI_CELL: f32 = 12.0;
    pub const MENU: f32 = 6.0;
    pub const MENU_ROW: f32 = 10.0;
    pub const BAR_CONTROL: f32 = 8.0;
    pub const MENU_PANEL: f32 = 16.0;
    pub const DIALOG_SYMBOL: f32 = 16.0;
    pub const DIALOG: f32 = 20.0;
    pub const THUMBNAIL: f32 = 6.0;
    pub const CARD: f32 = 10.0;
    pub const CODE_BLOCK: f32 = 14.0;
    pub const KEY_CAP: f32 = 6.0;
}

pub mod size {
    pub const PANEL_W: f32 = 750.0;
    pub const PANEL_H: f32 = 475.0;
    pub const HEADER_H: f32 = 36.0;
    pub const HEADER_ICON_SLOT: f32 = 22.0;
    pub const HEADER_PAD: f32 = 10.0;
    /// The header band the list underlaps.
    pub const HEADER_BAND: f32 = HEADER_H + HEADER_PAD * 2.0;
    pub const BOTTOM_BAR_H: f32 = 52.0;
    pub const BAR_BUTTON_H: f32 = 28.0;
    pub const RESULT_ROW_ICON: f32 = 26.0;
    pub const KEY_CAP: f32 = 18.0;
    pub const MENU_BUTTON: f32 = 36.0;
    pub const CLIPBOARD_LIST_W: f32 = 290.0;
    pub const EMOJI_GRID_INSET: f32 = 16.0;
    pub const MENU_W: f32 = 276.0;
    pub const ACTION_MENU_W: f32 = 320.0;
    pub const FILTER_MENU_W: f32 = 200.0;
    pub const EMOJI_CATEGORY_MENU_W: f32 = 220.0;
    pub const MENU_ICON: f32 = 20.0;
    pub const MENU_ROW_H: f32 = MENU_ICON + super::space::MD * 2.0;
    pub const MENU_ROW_GAP: f32 = 1.0;
    pub const MENU_SECTION_HEADER: f32 = 16.0;
    pub const MENU_VISIBLE_ROWS: f32 = 5.5;
    pub const DIALOG_COMPACT_W: f32 = 290.0;
    pub const DIALOG_W: f32 = 420.0;
    pub const DIALOG_BUTTON_H: f32 = 34.0;
    pub const DIALOG_SYMBOL: f32 = 28.0;
    pub const DIALOG_SYMBOL_BOX: f32 = 52.0;
    pub const HUD_MAX_W: f32 = 420.0;
    pub const HAIRLINE: f32 = 1.0;
    /// Fade bands of the edge dissolve: each overshoots its bar into the list.
    pub const DISSOLVE_TOP: f32 = HEADER_H + HEADER_PAD + 32.0;
    pub const DISSOLVE_BOTTOM: f32 = BOTTOM_BAR_H + 28.0;
}

/// The alpha ramp: white ink over the dark surface, black over the light one.
#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub panel_scrim: Color,
    pub selection: Color,
    pub row_hover: Color,
    pub emoji_cell: Color,
    pub emoji_hover_border: Color,
    pub emoji_selection_border: Color,
    pub menu_hover: Color,
    pub separator: Color,
    pub control_surface: Color,
    pub control_hover: Color,
    pub control_pressed: Color,
    pub border: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub text_tertiary: Color,
    pub menu_symbol: Color,
    pub icon_placeholder: Color,
    pub card_fill: Color,
    pub card_stroke: Color,
    pub dialog_dimming: Color,
    pub destructive: Color,
    pub success: Color,
    pub warning: Color,
    pub primary_action: Color,
    /// The body of a floating glass control: a tint over the blur behind it.
    pub glass_tint: Color,
    pub glass_rim_top: Color,
    pub glass_rim_bottom: Color,
    pub glass_shadow: Color,
    /// The panel's own hairline, lit on top as Liquid Glass lights its rim.
    pub panel_rim_top: Color,
    pub panel_rim_bottom: Color,
}

fn rgb(hex: u32, a: f32) -> Color {
    let c = rune::color::hex(&format!("#{hex:06x}")).unwrap_or(Color::WHITE);
    c.alpha(a)
}

pub fn palette(dark: bool) -> Palette {
    let ink = |d: f32, l: f32| if dark { Color::WHITE.alpha(d) } else { Color::BLACK.alpha(l) };
    Palette {
        dark,
        panel_scrim: if dark { Color::BLACK.alpha(0.40) } else { Color::WHITE.alpha(0.55) },
        selection: ink(0.10, 0.09),
        row_hover: ink(0.05, 0.045),
        emoji_cell: ink(0.045, 0.04),
        emoji_hover_border: ink(0.42, 0.34),
        emoji_selection_border: ink(0.92, 0.72),
        menu_hover: ink(0.10, 0.09),
        separator: ink(0.10, 0.12),
        control_surface: ink(0.10, 0.08),
        control_hover: ink(0.16, 0.14),
        control_pressed: ink(0.24, 0.20),
        border: ink(0.20, 0.18),
        text: ink(1.0, 1.0),
        text_secondary: ink(0.60, 0.60),
        text_tertiary: ink(0.40, 0.42),
        menu_symbol: ink(0.70, 0.70),
        icon_placeholder: ink(0.06, 0.06),
        card_fill: ink(0.05, 0.04),
        card_stroke: ink(0.10, 0.10),
        dialog_dimming: Color::BLACK.alpha(0.34),
        destructive: if dark { rgb(0xff453a, 1.0) } else { rgb(0xff3b30, 1.0) },
        success: if dark { rgb(0x30d158, 1.0) } else { rgb(0x34c759, 1.0) },
        warning: if dark { rgb(0xff9f0a, 1.0) } else { rgb(0xff9500, 1.0) },
        primary_action: if dark { rgb(0x0a84ff, 1.0) } else { rgb(0x007aff, 1.0) },
        glass_tint: if dark { rgb(0x3a3a3c, 0.46) } else { Color::WHITE.alpha(0.52) },
        glass_rim_top: if dark { Color::WHITE.alpha(0.30) } else { Color::WHITE.alpha(0.90) },
        glass_rim_bottom: if dark { Color::WHITE.alpha(0.07) } else { Color::BLACK.alpha(0.06) },
        glass_shadow: Color::BLACK.alpha(if dark { 0.30 } else { 0.14 }),
        panel_rim_top: if dark { Color::WHITE.alpha(0.24) } else { Color::WHITE.alpha(0.85) },
        panel_rim_bottom: if dark { Color::WHITE.alpha(0.08) } else { Color::BLACK.alpha(0.08) },
    }
}

/// Type. The sizes are macOS's text styles, which Tinycast uses by name.
pub mod type_ {
    use super::TextStyle;
    const fn s(size: f32, weight: f32, lh: f32) -> TextStyle {
        TextStyle::new(size, weight).lh(lh)
    }
    pub const SEARCH: TextStyle = s(20.0, 400.0, 26.0);
    /// `.body`
    pub const ROW_TITLE: TextStyle = s(13.0, 400.0, 18.0);
    /// `.callout`
    pub const ROW_TRAILING: TextStyle = s(12.0, 400.0, 16.0);
    /// `.subheadline.medium`
    pub const SECTION: TextStyle = s(11.0, 500.0, 14.0);
    /// `.headline`
    pub const PANEL_TITLE: TextStyle = s(13.0, 650.0, 18.0);
    /// `.title`
    pub const CALC_RESULT: TextStyle = s(22.0, 500.0, 28.0);
    /// `.caption`
    pub const KEY_CAP: TextStyle = s(10.0, 500.0, 13.0);
    /// `.callout.medium`
    pub const BAR: TextStyle = s(12.0, 550.0, 16.0);
    pub const MENU_ROW: TextStyle = s(13.0, 400.0, 18.0);
    pub const MENU_SHORTCUT: TextStyle = s(12.0, 400.0, 16.0);
    pub const CAPTION: TextStyle = s(10.0, 400.0, 13.0);
    pub const TITLE2: TextStyle = s(17.0, 650.0, 22.0);
    pub const TITLE3: TextStyle = s(15.0, 600.0, 20.0);
    pub const EMOJI: TextStyle = s(30.0, 400.0, 38.0);
    pub const EMOJI_SMALL: TextStyle = s(16.0, 400.0, 20.0);
}

pub mod motion {
    use super::Spring;
    /// Selection and hover: quick, no bounce.
    pub const HOVER: Spring = Spring::new(0.12, 0.0);
    pub const MENU: Spring = Spring::new(0.26, 0.12);
    pub const SCROLL: Spring = Spring::new(0.22, 0.0);
    pub const LAYOUT: Spring = Spring::new(0.30, 0.0);
}

pub mod duration {
    pub const MESSAGE_HUD: f64 = 2.4;
}
