//! Mobs: the `Mob` struct, kinds, tick order, spawning, `max_entities`, save and load (filled by
//! T9a; paths and AI by T9b, the mobs themselves by T9c).

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

/// The spawning step of the tick (`WorldEntitySpawner.findChunksForSpawning`), after the mobs tick.
pub fn spawn(_game: &mut Game) {}
