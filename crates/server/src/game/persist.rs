//! Save files: hands each module in `MODULES` its saved value at start and writes their `save`
//! values at the autosave interval and at shutdown. A module's value lives in `<module>.json` in
//! the data folder (`tiles.json` for the signs); a module whose `save` is null writes nothing.

use serde_json::Value;

use super::{Game, MODULES};

/// `MinecraftServer.tick` saves the players and worlds every 900 ticks (45 s).
pub const AUTOSAVE_TICKS: u64 = 900;

#[derive(Default)]
pub struct State {}

pub fn on_join(_game: &mut Game, _id: u32) {}

pub fn on_leave(_game: &mut Game, _id: u32) {}

pub fn on_frame(_game: &mut Game, _id: u32, _tag: u8, _body: &[u8]) -> bool {
    false
}

pub fn tick(game: &mut Game) {
    if game.ticks > 0 && game.ticks % AUTOSAVE_TICKS == 0 {
        save_all(game);
    }
}

pub fn save(_game: &Game) -> Value {
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}

/// When the game is made, before the first tick: read the files, call each module's `load`.
pub fn on_start(game: &mut Game) {
    for m in MODULES {
        let path = game.dir.join(format!("{}.json", m.name));
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        match serde_json::from_str::<Value>(&text) {
            Ok(v) => (m.load)(game, v),
            Err(e) => println!("{}: {e}; starting without it", path.display()),
        }
    }
}

/// At `Shutdown`, after the last tick: write everything.
pub fn on_shutdown(game: &mut Game) {
    save_all(game);
}

/// Writes every module's non-null `save` value to its file, each replaced whole.
pub fn save_all(game: &Game) {
    for m in MODULES {
        let v = (m.save)(game);
        if v.is_null() {
            continue;
        }
        let text = serde_json::to_string_pretty(&v).unwrap_or_default();
        crate::world::write_atomic(&game.dir.join(format!("{}.json", m.name)), &text);
    }
}
