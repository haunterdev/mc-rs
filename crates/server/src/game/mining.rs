//! `PlayerInteractionManager` on the server: `onBlockClicked` (`V_DIG` start), `blockRemoving`
//! with the 0.7 rule (`V_DIG` finish), `cancelDestroyingBlock` (abort), `updateBlockRemoving` as
//! [`tick`] and `tryHarvestBlock`. A broken block drops through `items::spawn_as_entity`, the
//! held tool wears, `Block.harvestBlock` adds 0.005 exhaustion, and the blocks a break turned
//! to air or liquid (a flower on broken dirt) drop what they held ([`neighbour_drops`]). Others
//! near the dig get `S_BREAK_ANIM` (`WorldServer.sendBlockBreakProgress`, 32 blocks, not the
//! breaker); a dig the server refuses is answered with `S_BLOCKS` holding its own state
//! (`NetHandlerPlayServer.processPlayerDigging`).
//!
//! The viewer's `seen` state is trusted when it differs from the server's and both are
//! breakable (population order can differ at chunk borders); each difference counts in
//! `stats.dig_mismatches`.
//!
//! DROPPED: `World.playEvent(2001)` (break sound and particles to the other players); statistics;
//! `Block.onBlockClicked` side effects (dragon egg teleport, note block, TNT); `onPlayerDestroy`
//! and `onBlockHarvested` except the double plant; command and
//! structure blocks (always refused: nobody here may use them); `ItemStack.canDestroy` tags, so an
//! adventure player breaks nothing; experience orbs and silverfish from `Drops`; the world
//! border and `isBlockProtected` (spawn protection); haste, fatigue and enchantments in the dig
//! speed (`DigContext`). Blocks turned to air by liquid flow or the server's own ticks drop
//! nothing, and a viewer's `V_BLOCKS` batch drops its neighbour breaks but not its first block.

use std::collections::HashMap;

use items::ids::{block, item};
use items::{DigContext, ItemStack};
use proto::{BlockChange, Dig, BreakAnim, DIG_ABORT, DIG_FINISH, DIG_START, ON_GROUND, SNEAKING};
use serde_json::Value;
use sim::mining::{block_hardness, player_break_block, player_relative_block_hardness};
use sim::region::{props, Mat};
use worldgen::java_random::JavaRandom;

use super::terrain;
use super::{items as item_entities, net, players, Game};

/// `NetHandlerPlayServer`: a dig further than this (squared) from the eyes is ignored.
const REACH_SQ: f64 = 36.0;
/// `WorldServer.sendBlockBreakProgress`: players within 32 blocks.
const ANIM_RANGE: f64 = 32.0;
/// `blockRemoving`: the share of the expected time a finish must have waited.
const FINISH_AT: f32 = 0.7;
const AIR: u16 = 0;
const BUILD_LIMIT: i32 = 256;

/// One player's `PlayerInteractionManager` digging fields.
#[derive(Default, Clone, Debug)]
pub struct Digging {
    /// `curblockDamage`: ticks this manager has run.
    pub cur: i32,
    pub is_destroying: bool,
    pub destroy_pos: (i32, i32, i32),
    /// The state the viewer saw at `destroy_pos`.
    seen: u16,
    pub initial_damage: i32,
    pub received_finish: bool,
    pub delayed_pos: (i32, i32, i32),
    delayed_seen: u16,
    pub initial_block_damage: i32,
    pub durability_remaining: i32,
}

#[derive(Default)]
pub struct State {
    pub players: HashMap<u32, Digging>,
    /// `World.rand` for the drop rolls.
    rand: Option<JavaRandom>,
    /// The server's states under a viewer's `V_BLOCKS` batch, read before it was applied.
    olds: Vec<u16>,
}

pub fn on_join(game: &mut Game, id: u32) {
    game.mining.players.insert(id, Digging { durability_remaining: -1, ..Default::default() });
}

pub fn on_leave(game: &mut Game, id: u32) {
    if let Some(d) = game.mining.players.remove(&id) {
        if d.is_destroying {
            anim(game, id, d.destroy_pos, -1);
        }
    }
}

fn decode_dig(body: &[u8]) -> Option<Dig> {
    Dig::decode(body)
}

/// `EnumFacing.getFrontOffsetX/Y/Z`: down, up, north, south, west, east.
fn face_offset(face: u8) -> (i32, i32, i32) {
    match face {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        _ => (1, 0, 0),
    }
}

/// `held.getItem() instanceof ItemSword`
fn is_sword(it: u16) -> bool {
    matches!(it, item::WOODEN_SWORD | item::STONE_SWORD | item::IRON_SWORD | item::GOLDEN_SWORD | item::DIAMOND_SWORD)
}

fn is_air(s: u16) -> bool {
    props(s).mat == Mat::Air
}

fn is_liquid(s: u16) -> bool {
    matches!(props(s).mat, Mat::Water | Mat::Lava)
}

/// `NetHandlerPlayServer.processPlayerDigging` for START, ABORT and STOP_DESTROY_BLOCK.
pub fn on_frame(game: &mut Game, id: u32, tag: u8, body: &[u8]) -> bool {
    if tag != proto::V_DIG {
        return false;
    }
    let Some(msg) = decode_dig(body) else { return true };
    let Some(p) = game.players.players.get(&id) else { return true };
    if p.dead {
        return true;
    }
    let Some(st) = game.sessions.get(&id).and_then(|s| s.state) else { return true };
    let at = (msg.pos[0], msg.pos[1], msg.pos[2]);
    let d0 = st.pos[0] - (at.0 as f64 + 0.5);
    let d1 = st.pos[1] - (at.1 as f64 + 0.5) + 1.5;
    let d2 = st.pos[2] - (at.2 as f64 + 0.5);
    if d0 * d0 + d1 * d1 + d2 * d2 > REACH_SQ || at.1 >= BUILD_LIMIT {
        return true;
    }
    if msg.state != terrain::state(game, at) {
        game.stats.dig_mismatches += 1;
    }
    match msg.action {
        DIG_START => on_block_clicked(game, id, at, msg.face, msg.state),
        DIG_ABORT => {
            cancel_destroying_block(game, id);
            resend_if_solid(game, id, at);
        }
        DIG_FINISH => {
            block_removing(game, id, at, msg.state);
            resend_if_solid(game, id, at);
        }
        _ => {}
    }
    true
}

/// `SPacketBlockChange(world, pos)` when the block is not air: the server's state to this player.
fn resend_if_solid(game: &Game, id: u32, at: (i32, i32, i32)) {
    let s = terrain::state(game, at);
    if !is_air(s) {
        net::send(game, id, proto::S_BLOCKS, &proto::encode_blocks(&[(at, s)]));
    }
}

/// The state a dig on `at` works with: the server's, or the viewer's `seen` when they differ and
/// both are breakable.
fn effective(game: &Game, at: (i32, i32, i32), seen: u16) -> u16 {
    let s = terrain::state(game, at);
    if s != seen && !is_air(s) && !is_air(seen) && block_hardness(s) >= 0.0 && block_hardness(seen) >= 0.0 {
        seen
    } else {
        s
    }
}

/// `EntityPlayer.getDigSpeed`'s conditions for this player.
pub(crate) fn dig_context(game: &Game, id: u32) -> DigContext {
    let st = game.sessions.get(&id).and_then(|s| s.state);
    let on_ground = st.map_or(true, |s| s.flags & ON_GROUND != 0);
    let eyes = st.is_some_and(|s| players::eye_in_water(game, s.pos, s.flags & SNEAKING != 0));
    let p = game.players.players.get(&id);
    let efficiency = p.map_or(0, |p| items::enchant::get_enchantment_level(items::enchant::id::EFFICIENCY, &p.inv.get_current_item()));
    let aqua_affinity = p.is_some_and(|p| items::enchant::get_max_enchantment_level(items::enchant::id::AQUA_AFFINITY, p.inv.armor.iter()) > 0);
    let haste = p.and_then(|p| p.effects.amplifier(items::potion::Effect::Haste));
    let mining_fatigue = p.and_then(|p| p.effects.amplifier(items::potion::Effect::MiningFatigue));
    DigContext { on_ground, eyes_in_water: eyes, efficiency, aqua_affinity, haste, mining_fatigue }
}

/// `IBlockState.getPlayerRelativeBlockHardness(player, world, pos)`
fn relative_hardness(game: &Game, id: u32, state: u16) -> f32 {
    let held = game.players.players.get(&id).map_or(ItemStack::EMPTY, |p| p.inv.get_current_item());
    player_relative_block_hardness(state, &held, &dig_context(game, id))
}

/// `World.sendBlockBreakProgress(breaker, pos, stage)`: to the other players within 32 blocks.
fn anim(game: &Game, id: u32, at: (i32, i32, i32), stage: i32) {
    let msg = BreakAnim { breaker: id, pos: [at.0, at.1, at.2], stage: stage.clamp(-1, 9) as i8 };
    net::send_near(game, Some(id), [at.0 as f64, at.1 as f64, at.2 as f64], ANIM_RANGE, proto::S_BREAK_ANIM, &msg.encode());
}

/// `World.extinguishFire(null, pos, side)`: a fire next to the clicked face goes out.
fn extinguish_fire(game: &mut Game, at: (i32, i32, i32), face: u8) -> bool {
    let o = face_offset(face);
    let f = (at.0 + o.0, at.1 + o.1, at.2 + o.2);
    if terrain::state(game, f) >> 4 == block::FIRE {
        terrain::set_block(game, f, AIR);
        return true;
    }
    false
}

/// `PlayerInteractionManager.onBlockClicked`
fn on_block_clicked(game: &mut Game, id: u32, at: (i32, i32, i32), face: u8, seen: u16) {
    let mode = players::mode(game, id);
    if mode == proto::GM_CREATIVE {
        if !extinguish_fire(game, at, face) {
            try_harvest(game, id, at, seen);
        }
        return;
    }
    // hasLimitedInteractions: spectator breaks nothing; adventure needs a canDestroy tag, which
    // no stack here carries.
    if mode != proto::GM_SURVIVAL {
        return;
    }
    extinguish_fire(game, at, face);
    let state = effective(game, at, seen);
    let Some(d) = game.mining.players.get(&id) else { return };
    let cur = d.cur;
    let mut f = 1.0f32;
    if !is_air(state) {
        f = relative_hardness(game, id, state);
    }
    if !is_air(state) && f >= 1.0 {
        try_harvest(game, id, at, seen);
        // initialDamage was set before the harvest.
        if let Some(d) = game.mining.players.get_mut(&id) {
            d.initial_damage = cur;
        }
    } else {
        let i = (f * 10.0) as i32;
        let d = game.mining.players.get_mut(&id).unwrap();
        d.initial_damage = cur;
        d.is_destroying = true;
        d.destroy_pos = at;
        d.seen = seen;
        d.durability_remaining = i;
        anim(game, id, at, i);
    }
}

/// `PlayerInteractionManager.blockRemoving`
fn block_removing(game: &mut Game, id: u32, at: (i32, i32, i32), seen: u16) {
    let Some(d) = game.mining.players.get(&id) else { return };
    if at != d.destroy_pos {
        return;
    }
    let i = d.cur - d.initial_damage;
    let initial = d.initial_damage;
    let received = d.received_finish;
    let state = effective(game, at, seen);
    if is_air(state) {
        return;
    }
    let f = relative_hardness(game, id, state) * (i + 1) as f32;
    if f >= FINISH_AT {
        game.mining.players.get_mut(&id).unwrap().is_destroying = false;
        anim(game, id, at, -1);
        try_harvest(game, id, at, seen);
    } else if !received {
        let d = game.mining.players.get_mut(&id).unwrap();
        d.is_destroying = false;
        d.received_finish = true;
        d.delayed_pos = at;
        d.delayed_seen = seen;
        d.initial_block_damage = initial;
    }
}

/// `PlayerInteractionManager.cancelDestroyingBlock`
fn cancel_destroying_block(game: &mut Game, id: u32) {
    let Some(d) = game.mining.players.get_mut(&id) else { return };
    d.is_destroying = false;
    let at = d.destroy_pos;
    anim(game, id, at, -1);
}

/// `PlayerInteractionManager.updateBlockRemoving`, for every player each tick.
pub fn tick(game: &mut Game) {
    let mut ids: Vec<u32> = game.mining.players.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        update_block_removing(game, id);
    }
}

fn update_block_removing(game: &mut Game, id: u32) {
    let d = game.mining.players.get_mut(&id).unwrap();
    d.cur += 1;
    let d = d.clone();
    if d.received_finish {
        let i = d.cur - d.initial_block_damage;
        let state = effective(game, d.delayed_pos, d.delayed_seen);
        if is_air(state) {
            game.mining.players.get_mut(&id).unwrap().received_finish = false;
        } else {
            let f = relative_hardness(game, id, state) * (i + 1) as f32;
            let j = (f * 10.0) as i32;
            if j != d.durability_remaining {
                anim(game, id, d.delayed_pos, j);
                game.mining.players.get_mut(&id).unwrap().durability_remaining = j;
            }
            if f >= 1.0 {
                game.mining.players.get_mut(&id).unwrap().received_finish = false;
                try_harvest(game, id, d.delayed_pos, d.delayed_seen);
            }
        }
    } else if d.is_destroying {
        let state = effective(game, d.destroy_pos, d.seen);
        if is_air(state) {
            anim(game, id, d.destroy_pos, -1);
            let m = game.mining.players.get_mut(&id).unwrap();
            m.durability_remaining = -1;
            m.is_destroying = false;
        } else {
            let k = d.cur - d.initial_damage;
            let f = relative_hardness(game, id, state) * (k + 1) as f32;
            let l = (f * 10.0) as i32;
            if l != d.durability_remaining {
                anim(game, id, d.destroy_pos, l);
                game.mining.players.get_mut(&id).unwrap().durability_remaining = l;
            }
        }
    }
}

fn rand(game: &mut Game) -> &mut JavaRandom {
    game.mining.rand.get_or_insert_with(|| JavaRandom::new(crate::config::random_u64() as i64))
}

/// `PlayerInteractionManager.tryHarvestBlock`: removes the block, then in survival wears the tool
/// and drops the harvest. False when nothing was removed.
pub fn try_harvest(game: &mut Game, id: u32, at: (i32, i32, i32), seen: u16) -> bool {
    let mode = players::mode(game, id);
    let state = effective(game, at, seen);
    let held = game.players.players.get(&id).map_or(ItemStack::EMPTY, |p| p.inv.get_current_item());
    let refuse = |game: &Game| resend_if_solid(game, id, at);
    // creative with a sword in hand breaks nothing
    if mode == proto::GM_CREATIVE && !held.is_empty() && is_sword(held.get_item()) {
        refuse(game);
        return false;
    }
    let b = state >> 4;
    if matches!(b, block::COMMAND_BLOCK | block::STRUCTURE_BLOCK | 210 | 211) {
        refuse(game);
        return false;
    }
    if mode == proto::GM_SPECTATOR || mode == proto::GM_ADVENTURE || is_air(state) {
        refuse(game);
        return false;
    }
    let creative = mode == proto::GM_CREATIVE;
    let mut drops: Vec<ItemStack> = Vec::new();
    // removeBlock: onBlockHarvested first. An upper double plant half takes the lower one with it.
    if b == block::DOUBLE_PLANT && state & 8 != 0 {
        let below = (at.0, at.1 - 1, at.2);
        let lower = terrain::state(game, below);
        if lower >> 4 == block::DOUBLE_PLANT {
            if !creative {
                let r = rand(game);
                drops.extend(items::harvest::double_plant_upper_harvested(lower, &held, r).stacks());
            }
            terrain::set_block(game, below, AIR);
        }
    }
    // BlockBed.onBlockHarvested: a creative player breaking the foot takes the head too.
    if b == block::BED && creative && state & 8 == 0 {
        let other = super::beds::other_half(state, at);
        if terrain::state(game, other) >> 4 == block::BED {
            terrain::set_block(game, other, AIR);
        }
    }
    let (changes, olds) = terrain::set_block_logged(game, at, AIR);
    if changes.is_empty() {
        refuse(game);
        return false;
    }
    // BlockBed.neighborChanged: the other half goes. A head losing its foot drops the bed
    // (`dropBlockAsItem`), a foot losing its head drops nothing (the head's harvest did).
    let mut bed_other = None;
    if b == block::BED {
        let other = super::beds::other_half(state, at);
        let os = terrain::state(game, other);
        if os >> 4 == block::BED {
            bed_other = Some(terrain::set_block_logged(game, other, AIR));
        }
    }
    if matches!(b, block::LOG | block::LOG2) {
        super::blockticks::log_broken(game, at);
    }
    let mut exhaustion = 0.0;
    let mut worn = false;
    let mut broke = false;
    let mut ice_melts = false;
    let mut xp = 0;
    if !creative {
        let slot = game.players.players.get(&id).map(|p| p.inv.current_item);
        let mut tool = held;
        let h = {
            let r = rand(game);
            player_break_block(state, &mut tool, r)
        };
        if let Some(slot) = slot {
            if let Some(p) = game.players.players.get_mut(&id) {
                if p.inv.get_current_item() != tool {
                    p.inv.main[slot] = if h.tool_broke { ItemStack::EMPTY } else { tool };
                    worn = true;
                }
            }
        }
        if h.tool_broke {
            broke = true;
        }
        drops.extend(h.drops.stacks());
        xp = h.drops.xp;
        exhaustion = h.exhaustion;
        ice_melts = h.ice_melts;
    }
    for stack in drops {
        item_entities::spawn_as_entity(game, at, stack);
    }
    super::xp_orbs::drop_xp_on_block_break(game, at, xp);
    if exhaustion > 0.0 {
        players::add_exhaustion(game, id, exhaustion);
    }
    if worn {
        players::send_slots(game, id);
    }
    if broke {
        // EntityLivingBase.renderBrokenItemStack: `playSound(ENTITY_ITEM_BREAK, 0.8F, 0.8F +
        // rand.nextFloat() * 0.4F)`, which the server sends to everyone but the breaker (whose own
        // client plays it). DROPPED: the 5 item break particles.
        if let Some(pos) = game.sessions.get(&id).and_then(|s| s.state.as_ref()).map(|st| st.pos) {
            let pitch = 0.8 + rand(game).next_float() * 0.4;
            super::mobs::sounds::play_sound_except(game, Some(id), pos, "entity.item.break", super::mobs::sounds::PLAYERS, 0.8, pitch);
        }
    }
    if ice_melts {
        terrain::set_block(game, at, block::FLOWING_WATER << 4);
    }
    // The block itself is the first change; what follows is the neighbour updates.
    neighbour_drops(game, &changes[1..], &olds[1..]);
    if let Some((c, o)) = bed_other {
        neighbour_drops(game, &c, &o);
    }
    true
}

/// The blocks a break or edit turned to air or liquid, each dropping what it held
/// (`Block.dropBlockAsItem`, fortune 0): a flower whose dirt went, a torch, a cactus column.
/// `olds` are the states `changes` replaced. Blocks that fall (sand) or are not items (fire) drop
/// nothing here.
pub fn neighbour_drops(game: &mut Game, changes: &[BlockChange], olds: &[u16]) {
    for (&(pos, now), &old) in changes.iter().zip(olds) {
        if !(is_air(now) || is_liquid(now)) || is_air(old) || is_liquid(old) {
            continue;
        }
        if matches!(old >> 4, block::SAND | block::GRAVEL | block::ANVIL | block::DRAGON_EGG | block::CONCRETE_POWDER | block::FIRE) {
            continue;
        }
        let stacks = {
            let r = rand(game);
            items::drops::drop_block_as_item(old, r).stacks()
        };
        for stack in stacks {
            item_entities::spawn_as_entity(game, pos, stack);
        }
    }
}

pub fn save(_game: &Game) -> Value {
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}

/// Before terrain.rs applies a session's batch: the server's states under it.
pub fn before_blocks(game: &mut Game, changes: &[BlockChange]) {
    let olds: Vec<u16> = changes.iter().map(|&(at, _)| terrain::state(game, at)).collect();
    game.mining.olds = olds;
}

/// A session's block changes, after terrain.rs took them: blocks the batch turned to air or
/// liquid drop what their neighbours held up. The first change is the player's own edit and
/// drops nothing (creative breaks), and `V_BLOCKS` placements replace plants without drops.
pub fn on_blocks(game: &mut Game, _id: u32, changes: &[BlockChange]) {
    let olds = std::mem::take(&mut game.mining.olds);
    if changes.len() > 1 && olds.len() == changes.len() {
        neighbour_drops(game, &changes[1..], &olds[1..]);
    }
}

#[cfg(test)]
#[path = "mining_tests.rs"]
mod tests;
