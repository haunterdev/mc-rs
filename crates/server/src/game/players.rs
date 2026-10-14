//! Per-player survival state: hunger, exhaustion, falling, air, lava and fire, the void, eating
//! (filled by T4).

use proto::PlayerState;
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

/// A session's new state, before it replaces `game.sessions[id].state`, so the old one is there
/// for the movement deltas.
pub fn on_state(_game: &mut Game, _id: u32, _state: &PlayerState) {}

/// The session's game mode changed (`/gamemode`); `game.sessions[id].mode` already has it.
pub fn on_game_mode(_game: &mut Game, _id: u32, _mode: u8) {}
