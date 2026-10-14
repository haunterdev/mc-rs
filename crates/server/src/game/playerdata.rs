//! `playerdata/<name>.json`: health, food, experience and inventory per player, loaded at join
//! and written when dirty, at leave and at shutdown (filled by T4).

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
