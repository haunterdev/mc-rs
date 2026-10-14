//! Save files: hands each module in `MODULES` its saved value at start and writes their `save`
//! values at the autosave interval and at shutdown (filled by T4 with playerdata, then T5 and
//! T9a with `entities.json`, T8 with `tiles.json`).

use serde_json::Value;

use super::Game;

#[derive(Default)]
pub struct State {}

pub fn on_join(_game: &mut Game, _id: u32) {}

pub fn on_leave(_game: &mut Game, _id: u32) {}

pub fn on_frame(_game: &mut Game, _id: u32, _tag: u8, _body: &[u8]) -> bool {
    false
}

pub fn tick(_game: &mut Game) {}

pub fn save(_game: &Game) -> Value {
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}

/// When the game is made, before the first tick: read the files, call each module's `load`.
pub fn on_start(_game: &mut Game) {}

/// At `Shutdown`, after the last tick: write everything.
pub fn on_shutdown(_game: &mut Game) {}
