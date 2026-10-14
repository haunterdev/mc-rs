//! `PlayerInteractionManager`: digging, harvest drops, tool wear, neighbour drops (filled by T6).

use proto::BlockChange;
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

/// A session's block changes, after terrain.rs took them: blocks a dig batch turned to air or
/// liquid drop what their neighbours held up.
pub fn on_blocks(_game: &mut Game, _id: u32, _changes: &[BlockChange]) {}
