//! Looks: art directions designed once, by a person, in Rust. A pack picks one by name or
//! inherits the user's; it never invents one. A Look owns the tokens, the material, and what
//! each intent turns into: packs say what something means, the Look says how it shows.

use rune::{Color, Spring};

use crate::theme::{Palette, Tint};

/// `glass` is EchoSpot's default, `ink` the flat brand Look, `ember` the expressive one.
pub const NAMES: &[&str] = &["glass", "ink", "ember"];

/// Names a pack may use for space, in points.
pub const SPACE: &[(&str, f32)] = &[("none", 0.0), ("xxs", 2.0), ("xs", 4.0), ("sm", 6.0), ("md", 8.0), ("lg", 10.0), ("xl", 12.0), ("xxl", 20.0)];
pub const RADIUS: &[(&str, f32)] = &[("row", 10.0), ("card", 10.0), ("panel", 26.0), ("control", 8.0), ("thumb", 6.0)];
/// Type roles: size, weight, line height.
pub const TYPE: &[(&str, f32, f32, f32)] = &[
    ("display", 28.0, 600.0, 34.0), ("title", 20.0, 600.0, 26.0), ("headline", 13.0, 650.0, 18.0), ("body", 13.0, 400.0, 18.0),
    ("caption", 11.0, 500.0, 14.0), ("mono", 12.0, 400.0, 17.0), ("tabular", 13.0, 500.0, 18.0),
];
pub const COLOURS: &[&str] = &["text", "text-secondary", "text-tertiary", "accent", "positive", "warning", "danger", "card", "separator"];

/// How an intent shows under a Look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntentStyle {
    /// The tint its mark and tag take; `None` leaves the Look's own text colour.
    pub tint: Option<Tint>,
    /// The rune-fx effect along the row's edge, by name; empty for none.
    pub effect: &'static str,
    /// Breathes: its mark fades in and out.
    pub pulse: bool,
    /// Lower contrast.
    pub quiet: bool,
    /// What a tag says when the row gives no accessory of its own.
    pub label: &'static str,
}

const PLAIN: IntentStyle = IntentStyle { tint: None, effect: "", pulse: false, quiet: false, label: "" };

pub struct Look {
    pub name: &'static str,
    pub about: &'static str,
    /// `glass` blurs what is behind it; `flat` is a solid fill.
    pub material: &'static str,
    /// The effect layers one surface may run at once.
    pub fx_budget: usize,
    /// Whether intents may bring a moving effect at all.
    pub effects: bool,
    /// The curve rows, cells and values move on when they come, go, reorder or change (§13.5).
    pub motion: Spring,
}

pub const LOOKS: &[Look] = &[
    Look { name: "glass", about: "Liquid glass over what is behind it: EchoSpot's own Look", material: "glass", fx_budget: 3, effects: true, motion: Spring::new(0.30, 0.0) },
    Look { name: "ink", about: "Flat black, off-white and orange: the brand, with no moving effects", material: "flat", fx_budget: 1, effects: false, motion: Spring::new(0.18, 0.0) },
    Look { name: "ember", about: "The expressive Look: stronger effects, for boards and overlays", material: "glass", fx_budget: 3, effects: true, motion: Spring::new(0.36, 0.22) },
];

pub fn get(name: &str) -> &'static Look {
    LOOKS.iter().find(|l| l.name == name).unwrap_or(&LOOKS[0])
}

impl Look {
    /// What an intent means here.
    pub fn intent(&self, intent: &str) -> IntentStyle {
        let fx = |name: &'static str| if self.effects { name } else { "" };
        match intent {
            "urgent" => IntentStyle { tint: Some(if self.name == "ink" { Tint::Orange } else { Tint::Red }), effect: fx("fire"), pulse: self.name == "ember", label: "Urgent", ..PLAIN },
            "live" => IntentStyle { tint: Some(Tint::Green), effect: fx("pulse"), pulse: true, label: "Live", ..PLAIN },
            "loading" => IntentStyle { tint: None, effect: fx("shimmer"), pulse: true, quiet: true, label: "Loading", ..PLAIN },
            "success" => IntentStyle { tint: Some(Tint::Green), effect: fx("sparkles"), label: "Done", ..PLAIN },
            "new" => IntentStyle { tint: Some(if self.name == "ink" { Tint::Orange } else { Tint::Blue }), effect: fx("glow"), label: "New", ..PLAIN },
            "warning" => IntentStyle { tint: Some(Tint::Yellow), label: "Warning", ..PLAIN },
            "muted" => IntentStyle { quiet: true, ..PLAIN },
            _ => PLAIN,
        }
    }

    /// The palette a surface under this Look draws with, from the theme's own.
    pub fn palette(&self, mut p: Palette) -> Palette {
        match self.name {
            "ink" => {
                let (ink, paper) = (rune::color::hex("#0d0d0d").unwrap_or(Color::BLACK), rune::color::hex("#f4f1ea").unwrap_or(Color::WHITE));
                let (text, ground) = if p.dark { (paper, ink) } else { (ink, paper) };
                p.text = text;
                p.text_secondary = text.alpha(0.62);
                p.text_tertiary = text.alpha(0.40);
                p.card_fill = text.alpha(0.06);
                p.card_stroke = text.alpha(0.14);
                p.panel_scrim = ground.alpha(0.97);
                p.primary_action = Tint::Orange.accent();
                p.selection = Tint::Orange.accent().alpha(0.22);
                p
            }
            "ember" => {
                p.primary_action = Tint::Orange.accent();
                p.selection = Tint::Orange.accent().alpha(0.20);
                p
            }
            _ => p,
        }
    }
}

/// A colour by its token name, or a tint's accent.
pub fn colour(p: &Palette, name: &str) -> Option<Color> {
    Some(match name {
        "text" => p.text,
        "text-secondary" => p.text_secondary,
        "text-tertiary" => p.text_tertiary,
        "accent" => p.primary_action,
        "positive" => Tint::Green.accent(),
        "warning" => Tint::Yellow.accent(),
        "danger" => p.destructive,
        "card" => p.card_fill,
        "separator" => p.separator,
        other => tint(other)?.accent(),
    })
}

pub fn tint(name: &str) -> Option<Tint> {
    Some(match name {
        "violet" => Tint::Violet,
        "indigo" => Tint::Indigo,
        "blue" => Tint::Blue,
        "cyan" => Tint::Cyan,
        "teal" => Tint::Teal,
        "green" => Tint::Green,
        "yellow" => Tint::Yellow,
        "orange" => Tint::Orange,
        "red" => Tint::Red,
        "pink" => Tint::Pink,
        "graphite" => Tint::Graphite,
        _ => return None,
    })
}

pub fn space(name: &str) -> Option<f32> {
    SPACE.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

pub fn radius(name: &str) -> Option<f32> {
    RADIUS.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

pub fn type_role(name: &str) -> Option<rune::TextStyle> {
    TYPE.iter().find(|(n, ..)| *n == name).map(|(n, size, weight, lh)| {
        let style = rune::TextStyle::new(*size, *weight).lh(*lh);
        if *n == "mono" { style.mono() } else { style }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_decides_what_an_intent_shows() {
        assert_eq!(get("glass").intent("urgent").effect, "fire");
        assert_eq!(get("ink").intent("urgent").effect, "");
        assert_eq!(get("ink").intent("urgent").tint, Some(Tint::Orange));
        assert!(get("glass").intent("muted").quiet);
        assert_eq!(get("nope").name, "glass");
        assert_eq!(NAMES, LOOKS.iter().map(|l| l.name).collect::<Vec<_>>().as_slice());
        assert_eq!((space("md"), radius("panel"), tint("teal").is_some(), tint("#ff0000").is_none()), (Some(8.0), Some(26.0), true, true));
    }
}
