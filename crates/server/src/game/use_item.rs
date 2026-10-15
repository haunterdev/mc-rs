//! `V_USE_ITEM`: `NetHandlerPlayServer.processTryUseItem`, `PlayerInteractionManager.processRightClick`
//! and `ItemStack.useItemRightClick` (a right click in the air), plus the active hand of
//! `EntityLivingBase` (`setActiveHand`, `updateActiveHand`, `onItemUseFinish`, `stopActiveHand`)
//! that food, milk and the bow run on.
//!
//! Adding an item that is used in the air is one entry in [`ITEMS`]: a test on the stack, its
//! `getMaxItemUseDuration`, and the three Java hooks (`onItemRightClick`, `onItemUseFinish`,
//! `onPlayerStoppedUsing`). Armor is the first entry (armor.rs), food (every `ItemFood`, soup and
//! golden apples through `items::food`), milk, the bow and the buckets follow. A `USE_START` frame
//! is a right click; `USE_RELEASE` is `RELEASE_USE_ITEM` (`stopActiveHand`).
//!
//! The active hand of each player lives in [`State`]: the hand, the stack and the ticks left
//! (`activeItemStackUseCount`), counted down once a tick by [`tick`]. The stack in the hand
//! changing item cancels it (`resetActiveHand`), and at 0 the item finishes.
//!
//! DROPPED: the cooldown tracker, the `HAND_STATES` data parameter (other players do not see the
//! eating pose), the eating particles (the server's `spawnParticle` is a no-op; the viewer makes
//! its own), `Item.onUsingTick`, and the statistics and advancement triggers. `ItemSnowball`,
//! `ItemEgg` and `ItemEnderPearl` throw entities the server does not have, so they have no entry.

use std::collections::HashMap;

use ::items::food;
use ::items::ids::item;
use ::items::{ItemStack, Kind};
use serde_json::Value;
use sim::raytrace::{self, Kind as HitKind};
use sim::region::{props, Mat};
use worldgen::populate::Facing;

use super::mobs::arrow::{self, Arrow, Pickup};
use super::mobs::sounds::{play_sound, PLAYERS};
use super::terrain::{self, Blocks};
use super::{armor, mining, players, use_on, Game};

/// `SoundCategory.BLOCKS`
const BLOCKS: u8 = 4;

/// `EnumActionResult`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Res {
    Success,
    Fail,
    Pass,
}

/// `EntityLivingBase.activeItemStack`, its hand and `activeItemStackUseCount`.
#[derive(Clone, Copy, Debug)]
pub struct Active {
    pub hand: u8,
    pub stack: ItemStack,
    pub count: i32,
}

#[derive(Default)]
pub struct State {
    /// Per session id.
    pub active: HashMap<u32, Active>,
}

/// `EnumAction`, for the sounds of `updateItemUse`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    None,
    Eat,
    Drink,
    Bow,
}

/// One item class: `Item.onItemRightClick`, `onItemUseFinish`, `onPlayerStoppedUsing`,
/// `getMaxItemUseDuration` and `getItemUseAction`.
struct ItemUse {
    test: fn(&ItemStack) -> bool,
    duration: fn(&ItemStack) -> i32,
    action: Action,
    /// `onItemRightClick(world, player, hand)`: the result and the stack to hold.
    right_click: fn(&mut Game, u32, u8, ItemStack) -> (Res, ItemStack),
    /// `onItemUseFinish(stack, world, entity)`: the stack to hold after.
    finish: fn(&mut Game, u32, ItemStack) -> ItemStack,
    /// `onPlayerStoppedUsing(stack, world, entity, timeLeft)` for the stack in `hand`.
    stopped: fn(&mut Game, u32, u8, i32),
}

fn no_finish(_: &mut Game, _: u32, s: ItemStack) -> ItemStack {
    s
}

fn no_stop(_: &mut Game, _: u32, _: u8, _: i32) {}

/// Every item with a use in the air, first match wins.
static ITEMS: &[ItemUse] = &[
    ItemUse { test: is_armor, duration: zero, action: Action::None, right_click: armor_right_click, finish: no_finish, stopped: no_stop },
    ItemUse { test: is_food, duration: food::max_item_use_duration, action: Action::Eat, right_click: food_right_click, finish: food_finish, stopped: no_stop },
    ItemUse { test: is_milk, duration: thirty_two, action: Action::Drink, right_click: milk_right_click, finish: milk_finish, stopped: no_stop },
    ItemUse { test: is_bow, duration: bow_duration, action: Action::Bow, right_click: bow_right_click, finish: no_finish, stopped: bow_stopped },
    ItemUse { test: is_bucket, duration: zero, action: Action::None, right_click: bucket_right_click, finish: no_finish, stopped: no_stop },
];

fn lookup(s: &ItemStack) -> Option<&'static ItemUse> {
    if s.is_empty() {
        return None;
    }
    ITEMS.iter().find(|u| (u.test)(s))
}

/// `ItemStack.getMaxItemUseDuration`: 0 for an item that is not held down.
pub fn max_item_use_duration(s: &ItemStack) -> i32 {
    lookup(s).map_or(0, |u| (u.duration)(s))
}

fn zero(_: &ItemStack) -> i32 {
    0
}

fn thirty_two(_: &ItemStack) -> i32 {
    32
}

pub fn on_join(_game: &mut Game, _id: u32) {}

pub fn on_leave(game: &mut Game, id: u32) {
    game.use_item.active.remove(&id);
}

pub fn save(_game: &Game) -> Value {
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}

pub fn on_frame(game: &mut Game, id: u32, tag: u8, body: &[u8]) -> bool {
    if tag != proto::V_USE_ITEM {
        return false;
    }
    if !game.sessions.get(&id).is_some_and(|s| s.survival_client) || !game.players.players.contains_key(&id) {
        return false;
    }
    let Some(m) = proto::UseItem::decode(body) else { return false };
    if m.phase == proto::USE_START {
        process_try_use_item(game, id, m.hand);
    } else {
        // processPlayerDigging: RELEASE_USE_ITEM
        stop_active_hand(game, id);
    }
    true
}

/// `NetHandlerPlayServer.processTryUseItem`.
fn process_try_use_item(game: &mut Game, id: u32, hand: u8) {
    let stack = held(game, id, hand);
    if !stack.is_empty() {
        process_right_click(game, id, hand, stack);
    }
}

fn held(game: &mut Game, id: u32, hand: u8) -> ItemStack {
    use_on::slot(game, id, hand).map_or(ItemStack::EMPTY, |s| *s)
}

/// `EntityLivingBase.setHeldItem`
fn set_held(game: &mut Game, id: u32, hand: u8, stack: ItemStack) {
    if let Some(s) = use_on::slot(game, id, hand) {
        *s = if stack.is_empty() { ItemStack::EMPTY } else { stack };
    }
}

/// `PlayerInteractionManager.processRightClick`. The slots go to the viewer through the
/// container diff of windows.rs, where Java calls `sendContainerToPlayer`.
fn process_right_click(game: &mut Game, id: u32, hand: u8, stack: ItemStack) {
    if players::mode(game, id) == proto::GM_SPECTATOR || game.players.players.get(&id).is_none_or(|p| p.dead) {
        return;
    }
    let Some(def) = lookup(&stack) else { return };
    let (res, mut new) = (def.right_click)(game, id, hand, stack);
    let max = max_item_use_duration(&new);
    if new == stack && max <= 0 {
        return;
    }
    if res == Res::Fail && max > 0 && !game.use_item.active.contains_key(&id) {
        return;
    }
    if players::mode(game, id) == proto::GM_CREATIVE {
        new.count = stack.count;
        if new.is_item_stack_damageable() {
            new.damage = stack.damage;
        }
    }
    set_held(game, id, hand, new);
}

/// `EntityLivingBase.setActiveHand`.
fn set_active_hand(game: &mut Game, id: u32, hand: u8) {
    let stack = held(game, id, hand);
    if !stack.is_empty() && !game.use_item.active.contains_key(&id) {
        game.use_item.active.insert(id, Active { hand, stack, count: max_item_use_duration(&stack) });
    }
}

/// `EntityLivingBase.stopActiveHand`: the item's `onPlayerStoppedUsing`, then `resetActiveHand`.
pub fn stop_active_hand(game: &mut Game, id: u32) {
    let Some(a) = game.use_item.active.get(&id).copied() else { return };
    if let Some(def) = lookup(&a.stack) {
        (def.stopped)(game, id, a.hand, a.count);
    }
    game.use_item.active.remove(&id);
}

/// The step of `EntityLivingBase.updateActiveHand` for every player using an item.
pub fn tick(game: &mut Game) {
    let mut ids: Vec<u32> = game.use_item.active.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        update_active_hand(game, id);
    }
}

fn update_active_hand(game: &mut Game, id: u32) {
    let Some(mut a) = game.use_item.active.get(&id).copied() else { return };
    if game.players.players.get(&id).is_none_or(|p| p.dead) {
        game.use_item.active.remove(&id);
        return;
    }
    let now = held(game, id, a.hand);
    // itemstack == this.activeItemStack: the stack in the hand is still the one in use.
    if now.is_empty() || !now.is_item_equal(&a.stack) {
        game.use_item.active.remove(&id);
        return;
    }
    a.stack = now;
    if a.count <= 25 && a.count % 4 == 0 {
        update_item_use(game, id, &a.stack);
    }
    a.count -= 1;
    if a.count == 0 {
        // onItemUseFinish
        update_item_use(game, id, &a.stack);
        game.use_item.active.remove(&id);
        if let Some(def) = lookup(&a.stack) {
            let out = (def.finish)(game, id, a.stack);
            set_held(game, id, a.hand, out);
        }
    } else {
        game.use_item.active.insert(id, a);
    }
}

/// The feet position of the session.
fn pos_of(game: &Game, id: u32) -> Option<[f64; 3]> {
    game.sessions.get(&id)?.state.map(|s| s.pos)
}

/// `EntityLivingBase.updateItemUse`: the drink and eat sounds.
/// DROPPED: the item crack particles of eating, which only the viewer spawns.
fn update_item_use(game: &mut Game, id: u32, stack: &ItemStack) {
    let Some(pos) = pos_of(game, id) else { return };
    let Some(def) = lookup(stack) else { return };
    let r = &mut game.players.rand;
    match def.action {
        Action::Drink => {
            let pitch = r.next_float() * 0.1 + 0.9;
            play_sound(game, pos, "entity.generic.drink", PLAYERS, 0.5, pitch);
        }
        Action::Eat => {
            let volume = 0.5 + 0.5 * r.next_int_bounded(2) as f32;
            let pitch = (r.next_float() - r.next_float()) * 0.2 + 1.0;
            play_sound(game, pos, "entity.generic.eat", PLAYERS, volume, pitch);
        }
        _ => {}
    }
}

// ---- armor ----

fn is_armor(s: &ItemStack) -> bool {
    matches!(s.def().kind, Kind::Armor(..))
}

/// `ItemArmor.onItemRightClick`: the piece goes to its slot and the stack in hand is emptied
/// (count 0, so a creative player gets it back from `processRightClick`).
fn armor_right_click(game: &mut Game, id: u32, hand: u8, stack: ItemStack) -> (Res, ItemStack) {
    if armor::right_click(game, id, hand) {
        let mut s = stack;
        s.count = 0;
        (Res::Success, s)
    } else {
        (Res::Fail, stack)
    }
}

// ---- food ----

fn is_food(s: &ItemStack) -> bool {
    food::food(s.get_item()).is_some()
}

/// `EntityPlayer.canEat`: a hungry player, or any for an always edible food. Vanilla also refuses
/// anyone who takes no damage, so creative (and spectator) cannot eat at all.
fn can_eat(game: &Game, id: u32, ignore_hunger: bool) -> bool {
    let mode = players::mode(game, id);
    let Some(p) = game.players.players.get(&id) else { return false };
    // CHANGED: creative eats at any hunger, by request.
    (ignore_hunger || p.food.need_food() || mode == proto::GM_CREATIVE) && mode != proto::GM_SPECTATOR
}

/// `ItemFood.onItemRightClick`
fn food_right_click(game: &mut Game, id: u32, hand: u8, stack: ItemStack) -> (Res, ItemStack) {
    let always = food::food(stack.get_item()).is_some_and(|f| f.always_edible);
    if can_eat(game, id, always) {
        set_active_hand(game, id, hand);
        (Res::Success, stack)
    } else {
        (Res::Fail, stack)
    }
}

/// `ItemFood.onItemUseFinish` (with `ItemSoup`'s bowl): `FoodStats.addStats`, the burp, then
/// `onFoodEaten` and the stack less one.
/// The effects `onFoodEaten` rolls (rotten flesh, raw chicken, pufferfish, spider eye, poisonous
/// potato, golden apples) are added to the player.
/// A creative eater skips `addStats` (the request behind [`can_eat`]); the effects still apply.
fn food_finish(game: &mut Game, id: u32, stack: ItemStack) -> ItemStack {
    let (Some(heal), Some(modifier)) = (food::heal_amount(&stack), food::saturation_modifier(&stack)) else { return stack };
    // CHANGED: creative eats at any hunger, by request; the hidden food and saturation numbers
    // do not change, but the effects below still apply.
    if players::mode(game, id) != proto::GM_CREATIVE {
        if let Some(p) = game.players.players.get_mut(&id) {
            p.food.add_stats(heal, modifier);
        }
    }
    if let Some(pos) = pos_of(game, id) {
        let pitch = game.players.rand.next_float() * 0.1 + 0.9;
        play_sound(game, pos, "entity.player.burp", PLAYERS, 0.5, pitch);
    }
    // ItemFood.onFoodEaten: the effects whose chance came up.
    for e in food::on_food_eaten(&stack, &mut game.players.rand) {
        super::effects::add(game, id, e);
    }
    food::eat(&stack)
}

// ---- milk ----

fn is_milk(s: &ItemStack) -> bool {
    s.get_item() == item::MILK_BUCKET
}

/// `ItemBucketMilk.onItemRightClick`
fn milk_right_click(game: &mut Game, id: u32, hand: u8, stack: ItemStack) -> (Res, ItemStack) {
    set_active_hand(game, id, hand);
    (Res::Success, stack)
}

/// `ItemBucketMilk.onItemUseFinish`.
fn milk_finish(game: &mut Game, id: u32, mut stack: ItemStack) -> ItemStack {
    // clearActivePotions
    super::effects::clear(game, id);
    if players::mode(game, id) != proto::GM_CREATIVE {
        stack.shrink(1);
    }
    if stack.is_empty() {
        ItemStack::of(item::BUCKET)
    } else {
        stack
    }
}

// ---- bow ----

const BOW_DURATION: i32 = 72000;

fn is_bow(s: &ItemStack) -> bool {
    s.get_item() == item::BOW
}

fn bow_duration(_: &ItemStack) -> i32 {
    BOW_DURATION
}

/// `ItemArrow`: the arrow, the spectral arrow and the tipped arrow.
fn is_arrow(s: &ItemStack) -> bool {
    !s.is_empty() && matches!(s.get_item(), item::ARROW | item::SPECTRAL_ARROW | item::TIPPED_ARROW)
}

/// `ItemBow.findAmmo`: the off hand, then the main hand, then the first arrow in the inventory.
/// Returns the `getSizeInventory` index.
fn find_ammo(game: &Game, id: u32) -> Option<usize> {
    let inv = &game.players.players.get(&id)?.inv;
    let off = inv.get_size_inventory() - 1;
    if is_arrow(&inv.get_stack_in_slot(off)) {
        return Some(off);
    }
    let cur = inv.current_item;
    if is_arrow(&inv.main[cur]) {
        return Some(cur);
    }
    (0..inv.get_size_inventory()).find(|&i| is_arrow(&inv.get_stack_in_slot(i)))
}

/// `ItemBow.onItemRightClick`
fn bow_right_click(game: &mut Game, id: u32, hand: u8, stack: ItemStack) -> (Res, ItemStack) {
    let flag = find_ammo(game, id).is_some();
    if players::mode(game, id) != proto::GM_CREATIVE && !flag {
        return (Res::Fail, stack);
    }
    set_active_hand(game, id, hand);
    (Res::Success, stack)
}

/// `ItemBow.getArrowVelocity`
pub fn get_arrow_velocity(charge: i32) -> f32 {
    let mut f = charge as f32 / 20.0;
    f = (f * f + f * 2.0) / 3.0;
    if f > 1.0 {
        f = 1.0;
    }
    f
}

/// `ItemBow.onPlayerStoppedUsing`.
/// DROPPED: the spectral and tipped arrow effects (every arrow flies as a plain one), the shooter's own motion added by
/// `EntityArrow.shoot(Entity, ...)` (the server does not track a player's velocity), and the
/// stats.
fn bow_stopped(game: &mut Game, id: u32, hand: u8, time_left: i32) {
    let creative = players::mode(game, id) == proto::GM_CREATIVE;
    let bow = use_on::slot(game, id, hand).map(|s| *s).unwrap_or(ItemStack::EMPTY);
    let flag = creative || bow.get_enchantment_level(items::enchant::id::INFINITY) > 0;
    let ammo = find_ammo(game, id);
    if ammo.is_none() && !flag {
        return;
    }
    let ammo_stack = match ammo {
        Some(i) => game.players.players[&id].inv.get_stack_in_slot(i),
        None => ItemStack::of(item::ARROW),
    };
    let f = get_arrow_velocity(BOW_DURATION - time_left);
    if (f as f64) < 0.1 {
        return;
    }
    let flag1 = flag && ammo_stack.get_item() == item::ARROW;
    let Some(state) = game.sessions.get(&id).and_then(|s| s.state) else { return };
    let sneaking = state.flags & proto::SNEAKING != 0;
    // new EntityArrow(world, shooter): at the eyes less 0.1.
    let mut a = Arrow::new(Some(id), [state.pos[0], state.pos[1] + players::eye_height(sneaking) as f64 - 0.100_000_001_490_116_12, state.pos[2]]);
    // EntityArrow.shoot(entity, pitch, yaw, 0.0F, f * 3.0F, 1.0F)
    let (pitch, yaw) = (state.pitch, state.yaw);
    let x = -(yaw * 0.017_453_292).sin() * (pitch * 0.017_453_292).cos();
    let y = -((pitch + 0.0) * 0.017_453_292).sin();
    let z = (yaw * 0.017_453_292).cos() * (pitch * 0.017_453_292).cos();
    a.shoot(x as f64, y as f64, z as f64, f * 3.0, 1.0, &mut game.players.rand);
    if f == 1.0 {
        a.critical = true;
    }
    let power = bow.get_enchantment_level(items::enchant::id::POWER);
    if power > 0 {
        a.damage += power as f64 * 0.5 + 0.5;
    }
    let punch = bow.get_enchantment_level(items::enchant::id::PUNCH);
    if punch > 0 {
        a.knockback = punch;
    }
    if bow.get_enchantment_level(items::enchant::id::FLAME) > 0 {
        a.fire = 100;
    }
    // Players' arrows can be picked up (EntityArrow's constructor), creative ones by creative only.
    a.pickup = if flag1 || creative && matches!(ammo_stack.get_item(), item::SPECTRAL_ARROW | item::TIPPED_ARROW) { Pickup::CreativeOnly } else { Pickup::Allowed };
    if let Some(s) = use_on::slot(game, id, hand) {
        if s.get_item() == item::BOW {
            s.damage_item(1, creative, &mut players::wear_rand());
        }
    }
    arrow::spawn(game, a);
    let pitch = 1.0 / (game.players.rand.next_float() * 0.4 + 1.2) + f * 0.5;
    play_sound(game, state.pos, "entity.arrow.shoot", PLAYERS, 1.0, pitch);
    if !flag1 && !creative {
        if let (Some(i), Some(p)) = (ammo, game.players.players.get_mut(&id)) {
            let mut s = p.inv.get_stack_in_slot(i);
            s.shrink(1);
            // deleteStack
            p.inv.set_inventory_slot_contents(i, if s.is_empty() { ItemStack::EMPTY } else { s });
        }
    }
}

// ---- buckets ----

fn is_bucket(s: &ItemStack) -> bool {
    matches!(s.get_item(), item::BUCKET | item::WATER_BUCKET | item::LAVA_BUCKET)
}

/// `Item.rayTrace(world, player, useLiquids)`: from the eyes 5 blocks along the look.
/// DROPPED: `ignoreBlockWithoutBoundingBox`, which a full bucket passes as true (the ray stops on
/// tall grass and flowers instead of passing through them).
fn ray_trace(game: &Game, id: u32, use_liquids: bool) -> Option<raytrace::Hit> {
    let st = game.sessions.get(&id)?.state?;
    let (f, f1) = (st.pitch, st.yaw);
    let eye = glam::DVec3::new(st.pos[0], st.pos[1] + players::eye_height(st.flags & proto::SNEAKING != 0) as f64, st.pos[2]);
    let f2 = (-f1 * 0.017_453_292 - std::f32::consts::PI).cos();
    let f3 = (-f1 * 0.017_453_292 - std::f32::consts::PI).sin();
    let f4 = -(-f * 0.017_453_292).cos();
    let f5 = (-f * 0.017_453_292).sin();
    let (f6, f7) = (f3 * f4, f2 * f4);
    let end = eye + glam::DVec3::new(f6 as f64 * 5.0, f5 as f64 * 5.0, f7 as f64 * 5.0);
    raytrace::ray_trace_blocks(&Blocks(game), eye, end, use_liquids, false)
}

/// `ItemBucket.onItemRightClick`. An empty bucket traces with liquids and fills from a still
/// source; a full one places its liquid against the face hit.
/// DROPPED: `isBlockModifiable` and `canPlayerEdit` (no spawn protection or adventure mode here).
fn bucket_right_click(game: &mut Game, id: u32, _hand: u8, stack: ItemStack) -> (Res, ItemStack) {
    let contained = match stack.get_item() {
        item::WATER_BUCKET => Some(worldgen::blocks::id::FLOWING_WATER),
        item::LAVA_BUCKET => Some(worldgen::blocks::id::FLOWING_LAVA),
        _ => None,
    };
    let flag = contained.is_none();
    let creative = players::mode(game, id) == proto::GM_CREATIVE;
    let Some(hit) = ray_trace(game, id, flag) else { return (Res::Pass, stack) };
    if hit.kind != HitKind::Block {
        return (Res::Pass, stack);
    }
    let pos = hit.pos;
    let Some(pp) = pos_of(game, id) else { return (Res::Pass, stack) };
    if flag {
        let s = terrain::state(game, pos);
        let mat = props(s).mat;
        if mat == Mat::Water && s & 15 == 0 {
            terrain::set_block(game, pos, 0);
            play_sound(game, pp, "item.bucket.fill", PLAYERS, 1.0, 1.0);
            (Res::Success, fill_bucket(game, id, stack, item::WATER_BUCKET, creative))
        } else if mat == Mat::Lava && s & 15 == 0 {
            play_sound(game, pp, "item.bucket.fill_lava", PLAYERS, 1.0, 1.0);
            terrain::set_block(game, pos, 0);
            (Res::Success, fill_bucket(game, id, stack, item::LAVA_BUCKET, creative))
        } else {
            (Res::Fail, stack)
        }
    } else {
        let flag1 = sim::placement::is_replaceable(&Blocks(game), pos);
        let pos1 = if flag1 && hit.face == Facing::Up {
            pos
        } else {
            let (ox, oy, oz) = hit.face.offset();
            (pos.0 + ox, pos.1 + oy, pos.2 + oz)
        };
        if try_place_contained_liquid(game, pos1, contained.unwrap_or(0)) {
            (Res::Success, if creative { stack } else { ItemStack::of(item::BUCKET) })
        } else {
            (Res::Fail, stack)
        }
    }
}

/// `ItemBucket.fillBucket`
fn fill_bucket(game: &mut Game, id: u32, mut empty: ItemStack, full: u16, creative: bool) -> ItemStack {
    if creative {
        return empty;
    }
    empty.shrink(1);
    if empty.is_empty() {
        return ItemStack::of(full);
    }
    let mut s = ItemStack::of(full);
    let added = game.players.players.get_mut(&id).is_some_and(|p| p.inv.add_item_stack_to_inventory(&mut s, false));
    if !added {
        // player.dropItem(stack, false)
        if let Some(p) = pos_of(game, id) {
            super::items::spawn_stack_at(game, [p[0], p[1] + 1.3, p[2]], ItemStack::of(full), None, sim::item_entity::DEFAULT_PICKUP_DELAY);
        }
    }
    empty
}

/// `ItemBucket.tryPlaceContainedLiquid`: a flowing source goes into air, a non-solid block or a
/// replaceable one (which drops), and the liquid ticks from there (terrain.rs wakes it).
/// DROPPED: the nether branch (`doesWaterVaporize`), the world has no such dimension here.
fn try_place_contained_liquid(game: &mut Game, pos: (i32, i32, i32), contained: u16) -> bool {
    if contained == 0 {
        return false;
    }
    let s = terrain::state(game, pos);
    let mat = props(s).mat;
    let flag = !matches!(mat, Mat::Solid | Mat::Ice);
    let flag1 = sim::placement::is_replaceable(&Blocks(game), pos);
    if s >> 4 != 0 && !flag && !flag1 {
        return false;
    }
    let lava = contained == worldgen::blocks::id::FLOWING_LAVA;
    let pp = pos_of_cell(pos);
    play_sound(game, pp, if lava { "item.bucket.empty_lava" } else { "item.bucket.empty" }, BLOCKS, 1.0, 1.0);
    // destroyBlock(pos, true) when a non-liquid is there, then setBlockState(pos, liquid, 11).
    let (changes, olds) = terrain::set_block_logged(game, pos, contained << 4);
    if !matches!(mat, Mat::Water | Mat::Lava) {
        mining::neighbour_drops(game, &changes, &olds);
    }
    true
}

fn pos_of_cell(p: (i32, i32, i32)) -> [f64; 3] {
    [p.0 as f64 + 0.5, p.1 as f64 + 0.5, p.2 as f64 + 0.5]
}
