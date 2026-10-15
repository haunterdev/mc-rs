//! Loot tables (`LootTable.generateLootForPools`, `LootPool`, `LootEntryItem`) read from the client
//! jar's `loot_tables/`, and an entity's death drops (`EntityLiving.dropLoot`).
//!
//! DROPPED: `empty` entries, the `set_damage`, `set_nbt`,
//! `enchant_with_levels`, `enchant_randomly`, `furnace_smelt` (its
//! condition, `on_fire`, is never true while mobs do not burn) and `set_attributes` functions; the
//! `entity_scores` and `killed_by_player` variants beyond the plain one; luck and bonus rolls
//! (luck is 0); `dropEquipment` and `deathLootTableSeed` (each mob has no equipment and no seed).

use std::collections::HashMap;

use items::registry;
use items::ItemStack;
use serde_json::Value;
use worldgen::java_random::JavaRandom;

use super::super::{items as item_entities, terrain, Game};

/// What the conditions read (`LootContext`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Ctx {
    /// `entity_properties` `on_fire`
    pub on_fire: bool,
    /// `killed_by_player`: `LootContext.getKillerPlayer() != null`
    pub killed_by_player: bool,
    /// `EnchantmentHelper.getLootingModifier(context.getKiller())`: the killer's held Looting.
    pub looting: i32,
}

/// A `RandomValueRange` from its JSON: a number, or an object with `min` and `max`.
fn range(v: Option<&Value>) -> (f32, f32) {
    match v {
        Some(Value::Object(o)) => (
            o.get("min").and_then(Value::as_f64).unwrap_or(0.0) as f32,
            o.get("max").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        ),
        Some(n) => {
            let f = n.as_f64().unwrap_or(0.0) as f32;
            (f, f)
        }
        None => (0.0, 0.0),
    }
}

/// `RandomValueRange.generateInt`: `MathHelper.getInt(rand, floor(min), floor(max))`.
fn generate_int(r: (f32, f32), rand: &mut JavaRandom) -> i32 {
    let (min, max) = (r.0.floor() as i32, r.1.floor() as i32);
    if min >= max {
        min
    } else {
        rand.next_int_bounded(max - min + 1) + min
    }
}

/// `LootConditionManager.testAllConditions`
fn test_all(conditions: Option<&Value>, rand: &mut JavaRandom, ctx: &Ctx) -> bool {
    let Some(list) = conditions.and_then(Value::as_array) else { return true };
    list.iter().all(|c| test(c, rand, ctx))
}

fn test(c: &Value, rand: &mut JavaRandom, ctx: &Ctx) -> bool {
    match c.get("condition").and_then(Value::as_str).unwrap_or("") {
        // KilledByPlayer
        "killed_by_player" => ctx.killed_by_player != c.get("inverse").and_then(Value::as_bool).unwrap_or(false),
        // RandomChance
        "random_chance" => rand.next_float() < c.get("chance").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        // RandomChanceWithLooting
        "random_chance_with_looting" => {
            let chance = c.get("chance").and_then(Value::as_f64).unwrap_or(0.0) as f32;
            let mult = c.get("looting_multiplier").and_then(Value::as_f64).unwrap_or(0.0) as f32;
            rand.next_float() < chance + ctx.looting as f32 * mult
        }
        // EntityHasProperty with OnFire
        "entity_properties" => {
            let want = c.get("properties").and_then(|p| p.get("on_fire")).and_then(Value::as_bool);
            want.is_none_or(|w| w == ctx.on_fire)
        }
        // DROPPED: every other condition fails.
        _ => false,
    }
}

/// `LootEntryItem.addLoot`
fn add_loot(entry: &Value, out: &mut Vec<ItemStack>, rand: &mut JavaRandom, ctx: &Ctx, nested: &Nested) {
    match entry.get("type").and_then(Value::as_str) {
        Some("item") => {}
        // LootEntryTable.addLoot: the other table's pools, rolled in place.
        Some("loot_table") => {
            let name = entry.get("name").and_then(Value::as_str).unwrap_or("");
            if let Some(t) = nested.get(&table_file(name)) {
                out.extend(generate_with(t, rand, ctx, nested));
            }
            return;
        }
        _ => return,
    }
    let Some(def) = entry.get("name").and_then(Value::as_str).and_then(registry::by_name) else { return };
    let mut stack = ItemStack::new(def.id, 1, 0);
    for f in entry.get("functions").and_then(Value::as_array).into_iter().flatten() {
        if !test_all(f.get("conditions"), rand, ctx) {
            continue;
        }
        match f.get("function").and_then(Value::as_str).map(|n| n.trim_start_matches("minecraft:")) {
            // SetCount.apply
            Some("set_count") => stack.set_count(generate_int(range(f.get("count")), rand)),
            // SetMetadata.apply: setItemDamage(metaRange.generateInt(rand))
            Some("set_data") => stack.set_item_damage(generate_int(range(f.get("data")), rand)),
            // LootingEnchantBonus.apply: no draw while the looting level is 0.
            Some("looting_enchant") => {
                if ctx.looting != 0 {
                    let (min, max) = range(f.get("count"));
                    // RandomValueRange.generateFloat: nextFloat * (max - min) + min, or min when equal
                    let g = if min >= max { min } else { rand.next_float() * (max - min) + min };
                    let grow = (ctx.looting as f32 * g).round() as i32;
                    stack.set_count(stack.get_count() + grow);
                    let limit = f.get("limit").and_then(Value::as_i64).unwrap_or(0) as i32;
                    if limit != 0 && stack.get_count() > limit {
                        stack.set_count(limit);
                    }
                }
            }
            _ => {}
        }
    }
    if stack.is_empty() {
        return;
    }
    if stack.get_count() < def.max_stack {
        out.push(stack);
    } else {
        let mut i = stack.get_count();
        while i > 0 {
            let mut s = stack.copy();
            s.set_count(i.min(stack.get_max_stack_size()));
            i -= s.get_count();
            out.push(s);
        }
    }
}

/// `LootPool.createLootRoll`
fn create_roll(pool: &Value, out: &mut Vec<ItemStack>, rand: &mut JavaRandom, ctx: &Ctx, nested: &Nested) {
    let mut list: Vec<(&Value, i32)> = Vec::new();
    let mut total = 0;
    for e in pool.get("entries").and_then(Value::as_array).into_iter().flatten() {
        if test_all(e.get("conditions"), rand, ctx) {
            // getEffectiveWeight(luck 0): max(floor(weight + quality * luck), 0)
            let w = e.get("weight").and_then(Value::as_f64).unwrap_or(1.0).floor() as i32;
            if w > 0 {
                list.push((e, w));
                total += w;
            }
        }
    }
    if total == 0 || list.is_empty() {
        return;
    }
    let mut k = rand.next_int_bounded(total);
    for (e, w) in list {
        k -= w;
        if k < 0 {
            add_loot(e, out, rand, ctx, nested);
            return;
        }
    }
}

/// `LootTable.generateLootForPools`
pub fn generate(table: &Value, rand: &mut JavaRandom, ctx: &Ctx) -> Vec<ItemStack> {
    generate_with(table, rand, ctx, &Nested::new())
}

/// The tables a `loot_table` entry can name, by jar path (see [`table_file`]).
pub type Nested = HashMap<String, Value>;

/// `entities/sheep` to `loot_tables/entities/sheep.json`
pub fn table_file(name: &str) -> String {
    format!("loot_tables/{}.json", name.trim_start_matches("minecraft:"))
}

/// [`generate`] with the tables its `loot_table` entries name already read.
pub fn generate_with(table: &Value, rand: &mut JavaRandom, ctx: &Ctx, nested: &Nested) -> Vec<ItemStack> {
    let mut out = Vec::new();
    for pool in table.get("pools").and_then(Value::as_array).into_iter().flatten() {
        // LootPool.generateLoot: rolls + floor(bonus_rolls * luck), luck 0.
        if !test_all(pool.get("conditions"), rand, ctx) {
            continue;
        }
        let rolls = generate_int(range(pool.get("rolls")), rand);
        for _ in 0..rolls {
            create_roll(pool, &mut out, rand, ctx, nested);
        }
    }
    out
}

/// The loot table path for a mob kind (`getLootTable`). A sheep's depends on its fleece colour
/// and shearing, see [`Mob::loot_table_path`](super::Mob::loot_table_path).
pub fn table_path(kind: &str) -> String {
    format!("loot_tables/entities/{kind}.json")
}

/// The parsed table at `path` in the client jar, read once.
pub fn table_at(game: &mut Game, path: &str) -> Option<Value> {
    if !game.mobs.loot.contains_key(path) {
        let jar = if game.cfg.jar.is_empty() { jar::Jar::find() } else { jar::Jar::open(std::path::Path::new(&game.cfg.jar)) };
        let v = jar.ok().and_then(|j| j.json(path).ok());
        game.mobs.loot.insert(path.to_string(), v);
    }
    game.mobs.loot.get(path).cloned().flatten()
}

/// Every table `table` names through `loot_table` entries, read from the jar, nested ones too.
fn load_nested(game: &mut Game, table: &Value, out: &mut Nested) {
    for pool in table.get("pools").and_then(Value::as_array).into_iter().flatten() {
        for e in pool.get("entries").and_then(Value::as_array).into_iter().flatten() {
            if e.get("type").and_then(Value::as_str) == Some("loot_table") {
                let path = table_file(e.get("name").and_then(Value::as_str).unwrap_or(""));
                if !out.contains_key(&path) {
                    if let Some(t) = table_at(game, &path) {
                        out.insert(path, t.clone());
                        load_nested(game, &t, out);
                    }
                }
            }
        }
    }
}

/// `EntityLiving.dropLoot`: the table at the jar path `path` rolled with the mobs' generator, each
/// stack through `entityDropItem(stack, 0)` (an item entity at the mob with the default pickup
/// delay). Only while `doMobLoot` is on (`EntityLivingBase.onDeath`).
pub fn drop_loot(game: &mut Game, path: &str, pos: [f64; 3], ctx: Ctx) -> Vec<u32> {
    if !terrain::rule(game, "doMobLoot", true) {
        return Vec::new();
    }
    let Some(t) = table_at(game, path) else { return Vec::new() };
    let mut nested = Nested::new();
    load_nested(game, &t, &mut nested);
    let stacks = generate_with(&t, &mut game.mobs.rand, &ctx, &nested);
    stacks.into_iter().map(|s| item_entities::spawn_stack_at(game, pos, s, None, sim::item_entity::DEFAULT_PICKUP_DELAY)).collect()
}
