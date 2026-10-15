//! `playerdata/<lowercase name>.json`: health, food, air, fall distance, experience, the selected
//! slot and the inventory of each player (`EntityPlayer.writeEntityToNBT`, one file per player as
//! vanilla's `playerdata/<uuid>.dat`). Loaded when the player joins (`players::on_join` calls
//! [`apply`] right after it makes the `Survival`), written at most every 5 s and only when the
//! content changed, at leave ([`write`] from `players::on_leave`, before the `Survival` is gone),
//! on `GameMsg::Save` and at shutdown (both through [`save`]).
//!
//! The game mode, position, rotation and spawn point stay in `players.json` (world.rs), which the
//! hub keeps. Item ids are registry names, so a change to the numeric table never corrupts a save.
//! A player saved dead comes back alive on a fresh `Survival`: the inventory was dropped at death.

use std::collections::HashMap;

use items::{ItemStack, Tag};
use serde_json::{json, Value};

use super::players::Survival;
use super::Game;

/// The folder in the data folder that holds the files.
pub const DIR: &str = "playerdata";
/// Ticks between writes of changed files (5 s).
pub const WRITE_TICKS: u64 = 100;
const VERSION: i64 = 1;

#[derive(Default)]
pub struct State {
    /// The text last written per file name, so an unchanged player writes nothing.
    written: HashMap<String, String>,
}

/// The file name for a player: lower case, anything outside `a-z 0-9 _ -` made `_`.
pub fn file_name(name: &str) -> String {
    let s: String = name.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    format!("{s}.json")
}

pub(super) fn stack_json(slot: i32, s: &ItemStack) -> Option<Value> {
    let def = items::registry::item(s.item)?;
    let mut v = json!({"slot": slot, "id": format!("minecraft:{}", def.name), "count": s.count, "meta": s.damage});
    let tag = match s.tag {
        Tag::None => None,
        Tag::Potion(p) => Some([1, p, 0]),
        Tag::Entity(e) => Some([3, e, 0]),
    };
    if let Some(t) = tag {
        v["tag"] = json!(t);
    }
    if !s.ench.is_empty() {
        v["ench"] = json!(s.ench.iter().map(|(i, l)| [i, l]).collect::<Vec<_>>());
    }
    Some(v)
}

pub(super) fn stack_from(v: &Value) -> Option<(i32, ItemStack)> {
    let slot = v.get("slot")?.as_i64()? as i32;
    let def = items::registry::by_name(v.get("id")?.as_str()?)?;
    let count = v.get("count")?.as_i64()? as i32;
    let meta = v.get("meta").and_then(Value::as_i64).unwrap_or(0) as i32;
    let mut s = ItemStack::new(def.id, count, meta);
    if let Some(t) = v.get("tag").and_then(Value::as_array) {
        let b = |i: usize| t.get(i).and_then(Value::as_u64).unwrap_or(0) as u8;
        s.tag = match b(0) {
            1 => Tag::Potion(b(1)),
            3 => Tag::Entity(b(1)),
            _ => Tag::None,
        };
    }
    if let Some(e) = v.get("ench").and_then(Value::as_array) {
        let pair = |p: &Value| {
            let a = p.as_array()?;
            Some((a.first()?.as_u64()? as u8, a.get(1)?.as_u64()? as u8))
        };
        s.ench = items::EnchList::from_pairs(e.iter().filter_map(pair));
    }
    Some((slot, s))
}

/// One player's file.
pub fn to_json(p: &Survival) -> Value {
    let inventory: Vec<Value> = p.inv.to_save().iter().filter_map(|(slot, s)| stack_json(*slot, s)).collect();
    json!({
        "version": VERSION,
        "health": p.health,
        "food": p.food.level,
        "saturation": p.food.saturation,
        "exhaustion": p.food.exhaustion,
        "food_timer": p.food.timer,
        "air": p.air,
        "absorption": p.absorption,
        "effects": super::effects::effects_json(&p.effects),
        "fire": p.fire,
        "fall_distance": p.fall_distance,
        "dead": p.dead,
        "selected": p.inv.current_item,
        "xp": {"level": p.xp.level, "total": p.xp.total, "bar": p.xp.bar},
        "xp_seed": p.xp_seed,
        "inventory": inventory,
    })
}

/// Reads a file into `p`. False (and `p` untouched) for a file of another version, one that is
/// not an object, or a player saved dead.
pub fn from_json(p: &mut Survival, v: &Value) -> bool {
    if v.get("version").and_then(Value::as_i64) != Some(VERSION) || v.get("dead").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    let f = |k: &str| v.get(k).and_then(Value::as_f64);
    let i = |k: &str| v.get(k).and_then(Value::as_i64);
    // The effects come first: the health boost sets the maximum that set_health clamps to.
    super::effects::read_effects(&mut p.effects, v.get("effects"));
    p.max_health = super::players::MAX_HEALTH + p.effects.max_health_bonus() as f32;
    if let Some(a) = f("absorption").filter(|a| a.is_finite() && *a >= 0.0) {
        p.absorption = a as f32;
    }
    if let Some(h) = f("health").filter(|h| h.is_finite()) {
        p.set_health((h as f32).max(1.0));
    }
    if let Some(n) = i("food") {
        p.food.level = n.clamp(0, 20) as i32;
        p.food.prev_level = p.food.level;
    }
    if let Some(s) = f("saturation").filter(|s| s.is_finite()) {
        p.food.saturation = (s as f32).clamp(0.0, 20.0);
    }
    if let Some(e) = f("exhaustion").filter(|e| e.is_finite()) {
        p.food.exhaustion = (e as f32).clamp(0.0, 40.0);
    }
    if let Some(t) = i("food_timer") {
        p.food.timer = t.clamp(0, 80) as i32;
    }
    if let Some(a) = i("air") {
        p.air = a.clamp(-20, super::players::AIR_MAX as i64) as i32;
    }
    if let Some(n) = i("fire") {
        p.fire = n.clamp(0, 1_000_000) as i32;
    }
    if let Some(d) = f("fall_distance").filter(|d| d.is_finite()) {
        p.fall_distance = (d as f32).max(0.0);
    }
    if let Some(x) = v.get("xp") {
        let xf = |k: &str| x.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        p.xp = proto::Xp { bar: (xf("bar") as f32).clamp(0.0, 1.0), level: xf("level").max(0.0) as i32, total: xf("total").max(0.0) as i32 };
    }
    // readEntityFromNBT: a missing or zero XpSeed keeps the one drawn for a new player.
    if let Some(n) = i("xp_seed").filter(|&n| n != 0) {
        p.xp_seed = n as i32;
    }
    if let Some(list) = v.get("inventory").and_then(Value::as_array) {
        let stacks: Vec<(i32, ItemStack)> = list.iter().filter_map(stack_from).collect();
        p.inv.from_save(&stacks);
    }
    p.inv.current_item = i("selected").unwrap_or(0).clamp(0, 8) as usize;
    true
}

/// Gives the joining player what their file holds, if there is one. Called by `players::on_join`.
pub fn apply(game: &mut Game, id: u32) {
    let Some(name) = game.sessions.get(&id).map(|s| s.name.clone()) else { return };
    let path = game.dir.join(DIR).join(file_name(&name));
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let v = match serde_json::from_str::<Value>(&text) {
        Ok(v) => v,
        Err(e) => {
            println!("{}: {e}; starting {name} fresh", path.display());
            return;
        }
    };
    if let Some(p) = game.players.players.get_mut(&id) {
        let mut loaded = Survival::new();
        if from_json(&mut loaded, &v) {
            loaded.respawn_invulnerable = p.respawn_invulnerable;
            *p = loaded;
        }
    }
}

fn write_one(game: &mut Game, name: &str, v: &Value) {
    let file = file_name(name);
    let text = serde_json::to_string_pretty(v).unwrap_or_default();
    if game.playerdata.written.get(&file).is_some_and(|w| *w == text) {
        return;
    }
    let dir = game.dir.join(DIR);
    let _ = std::fs::create_dir_all(&dir);
    crate::world::write_atomic(&dir.join(&file), &text);
    game.playerdata.written.insert(file, text);
}

/// Writes player `id`'s file if it changed.
pub fn write(game: &mut Game, id: u32) {
    let Some(name) = game.sessions.get(&id).map(|s| s.name.clone()) else { return };
    let Some(v) = game.players.players.get(&id).map(to_json) else { return };
    write_one(game, &name, &v);
}

fn write_all(game: &mut Game) {
    let ids: Vec<u32> = game.players.players.keys().copied().collect();
    for id in ids {
        write(game, id);
    }
}

pub fn on_join(_game: &mut Game, _id: u32) {}

pub fn on_leave(_game: &mut Game, _id: u32) {}

pub fn on_frame(_game: &mut Game, _id: u32, _tag: u8, _body: &[u8]) -> bool {
    false
}

pub fn tick(game: &mut Game) {
    if game.ticks > 0 && game.ticks % WRITE_TICKS == 0 {
        write_all(game);
    }
}

/// Called by `persist::save_all` (a backup, the autosave, shutdown): every online player's file.
/// The module has no single file of its own, so the value is null.
pub fn save(game: &Game) -> Value {
    // `save` takes `&Game`, so the changed-only cache cannot be updated here; write whole files.
    let dir = game.dir.join(DIR);
    let _ = std::fs::create_dir_all(&dir);
    for (id, p) in &game.players.players {
        if let Some(s) = game.sessions.get(id) {
            let text = serde_json::to_string_pretty(&to_json(p)).unwrap_or_default();
            crate::world::write_atomic(&dir.join(file_name(&s.name)), &text);
        }
    }
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}
