//! Raycast extensions. (Runtime bridge lands next; this is the surface the palette uses.)

use rune::Tx;

use crate::app::App;
use crate::model::Item;

pub mod store;
pub mod view;

#[derive(Clone, Debug)]
pub enum ExtAct {
    None,
}

#[derive(Default)]
pub struct Extensions {}

impl Extensions {
    pub fn load() -> Self {
        Self::default()
    }
    pub fn pop_view(&mut self, _session: u64) -> bool {
        false
    }
    pub fn stop(&mut self, _session: u64) {}
}

pub fn perform(_app: &mut App, _act: ExtAct, _tx: &mut Tx) {}

pub fn commands(_app: &App) -> Vec<(Item, String)> {
    Vec::new()
}

pub fn save_preferences(_app: &mut App, _extension: &str, _then: Option<String>, _tx: &mut Tx) {}

pub fn submit_form(_app: &mut App, _session: u64, _tx: &mut Tx) {}

pub fn open_form_actions(_app: &mut App, _session: u64) {}
