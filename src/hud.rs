//! The message pill: a transient confirmation near the bottom of the screen, for an action whose
//! effect is otherwise invisible once the palette has gone ("Copied to Clipboard").

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use rune::{Cx, El, OverlayHandle, el, icon, row, text};

use crate::app::Tone;
use crate::icons;
use crate::theme::{self, size, space, type_};

#[derive(Default)]
pub struct Hud {
    pub text: String,
    pub tone: Option<Tone>,
}

static HANDLE: OnceLock<OverlayHandle<Hud>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub const SIZE: (f32, f32) = (size::HUD_MAX_W, 64.0);
pub const EDGE_OFFSET: f32 = 48.0;

pub fn attach(handle: OverlayHandle<Hud>) {
    let _ = HANDLE.set(handle);
}

/// Shows the pill; a repeat replaces what is up rather than stacking.
pub fn show(message: &str, tone: Tone) {
    let Some(handle) = HANDLE.get() else { return };
    let message = message.to_string();
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    handle.update(move |hud, _| {
        hud.text = message;
        hud.tone = Some(tone);
    });
    handle.show();
    let handle = handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs_f64(theme::duration::MESSAGE_HUD));
        if GENERATION.load(Ordering::SeqCst) == generation {
            handle.update(|hud, _| hud.tone = None);
            std::thread::sleep(Duration::from_millis(220));
            if GENERATION.load(Ordering::SeqCst) == generation {
                handle.hide();
            }
        }
    });
}

pub fn tone_glyph(tone: Tone, p: &theme::Palette) -> El {
    let (glyph, color) = match tone {
        Tone::Success => (&icons::CHECK, p.success),
        Tone::Danger => (&icons::ALERT, p.destructive),
        Tone::Neutral => (&icons::INFO, p.text_secondary),
        Tone::Progress => (&icons::REFRESH, p.primary_action),
    };
    let glyph = icon(glyph, 14.0).color(color);
    if tone == Tone::Progress { glyph.looping(rune::LoopSpec::rotation(0.9)) } else { glyph }
}

pub fn view(hud: &Hud, _: &mut Cx<Hud>) -> El {
    let p = theme::palette(true);
    let mut root = row().w_full().h_full().center();
    if let Some(tone) = hud.tone {
        let pill = row().key("pill").h(36.0).px(space::XL + 2.0).gap(space::MD).center_y().radius(18.0)
            .fill(rune::color::hex("#1c1c1e").unwrap_or(rune::Color::BLACK).alpha(0.86))
            .border_gradient(1.0, p.glass_rim_top, p.glass_rim_bottom)
            .shadow(6.0, 22.0, rune::Color::BLACK.alpha(0.35))
            .transition(rune::Transition::opacity().combined(rune::Transition::offset(0.0, 6.0)))
            .child(tone_glyph(tone, &p))
            .child(text(hud.text.clone()).font(type_::BAR).color(p.text));
        root = root.child(pill);
    }
    el().w_full().h_full().child(root)
}
