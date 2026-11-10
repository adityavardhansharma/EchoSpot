use crate::app::{App, Built};
use crate::model::Sections;

pub fn build(_app: &App, _query: &str) -> Built {
    Built::list(Sections::default(), "Search the extension store...").empty("Store", "Coming up.")
}

pub fn installed(_app: &App, _query: &str) -> Built {
    Built::list(Sections::default(), "Search installed extensions...").empty("No Extensions Installed", "")
}
