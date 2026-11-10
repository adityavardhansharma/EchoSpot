use rune::{El, text};

use crate::app::{App, Built};
use crate::model::Sections;
use crate::theme::{Palette, type_};

pub fn build(_app: &App, _session: u64, _query: &str) -> Built {
    Built::list(Sections::default(), "")
}

/// A body the generic list can't draw (a detail view); `None` leaves the list.
pub fn body(_app: &App, _p: &Palette, _scale: f32, _session: u64) -> Option<El> {
    None
}

pub fn markdown(p: &Palette, source: &str) -> El {
    text(source.to_string()).font(type_::ROW_TITLE).color(p.text).wrap()
}
