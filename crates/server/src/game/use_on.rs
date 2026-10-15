//! `V_USE_ON`: `NetHandlerPlayServer.processTryUseItemOnBlock` and
//! `PlayerInteractionManager.processRightClickBlock`. A right click on a block goes through the
//! server like vanilla: the block's own use first (windows.rs opens the crafting table, chest and
//! furnace; a sneaking player with an item skips it), else `ItemStack.onItemUse` through [`USES`].
//!
//! Adding an item that acts on a block (hoe, seeds, bone meal, bed) is one entry in [`USES`]: a
//! test on the stack and a function that returns whether the item was used. The hoe, seeds and
//! bone meal are farming.rs, the bed is beds.rs. `V_USE_ITEM` (right click in the air) is
//! use_item.rs.
//!
//! DROPPED: `canPlayerEdit` (adventure mode and spawn protection do not exist here), the world
//! border, and `Block.onBlockActivated` for the blocks the server has no use for yet (the doors
//! are doors.rs; signs and slabs are still the viewer's own, shared as `V_BLOCKS`).

use ::items::ids::block as bid;
use ::items::{ItemStack, Kind, Tag};
use proto::UseOn;
use serde_json::Value;
use sim::placement::{is_replaceable, may_place_among};
use worldgen::blocks::id;
use worldgen::populate::Facing;

use super::terrain::{self, Blocks};
use super::{beds, doors, farming, jukebox, mobs, net, players, windows, Game};

/// `processTryUseItemOnBlock`'s reach, squared: 6 blocks.
const REACH_SQ: f64 = 36.0;

/// What an item did on the block.
pub enum Used {
    /// `EnumActionResult.SUCCESS`; `consume` is the `itemstack.shrink(1)` outside creative.
    Success { consume: bool },
    /// `FAIL` or `PASS`: nothing happened.
    No,
}

type UseFn = fn(&mut Game, u32, &UseOn, ItemStack) -> Used;

/// Every item with an `onItemUse`, first match wins.
static USES: &[(fn(&ItemStack) -> bool, UseFn)] = &[
    (places_block, item_block_use),
    (is_spawn_egg, spawn_egg_use),
    (beds::is_bed_item, beds::item_use),
    (farming::is_hoe, farming::hoe_use),
    (farming::is_seed, farming::seed_use),
    (farming::is_bone_meal, farming::bone_meal_use),
    (doors::is_door_item, doors::item_use),
    (is_record, jukebox::item_use),
];

fn is_record(s: &ItemStack) -> bool {
    jukebox::is_record_item(s)
}

fn places_block(s: &ItemStack) -> bool {
    placed_block(s).is_some()
}

fn is_spawn_egg(s: &ItemStack) -> bool {
    s.item == ::items::ids::item::SPAWN_EGG
}

/// The block a block item places (`ItemBlock.block`, `ItemBlockSpecial.block`).
fn placed_block(s: &ItemStack) -> Option<u16> {
    if s.is_empty() {
        return None;
    }
    match s.def().kind {
        Kind::Block { block, .. } | Kind::Places(block) => Some(block),
        _ => None,
    }
}

pub fn on_join(_game: &mut Game, _id: u32) {}

pub fn on_leave(_game: &mut Game, _id: u32) {}

pub fn tick(_game: &mut Game) {}

pub fn save(_game: &Game) -> Value {
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}

pub fn on_frame(game: &mut Game, id: u32, tag: u8, body: &[u8]) -> bool {
    if tag != proto::V_USE_ON {
        return false;
    }
    if !game.sessions.get(&id).is_some_and(|s| s.survival_client) || !game.players.players.contains_key(&id) {
        return false;
    }
    let Some(m) = UseOn::decode(body) else { return false };
    process_right_click_block(game, id, &m);
    true
}

/// `PlayerInteractionManager.processRightClickBlock`.
fn process_right_click_block(game: &mut Game, id: u32, m: &UseOn) {
    if players::mode(game, id) == proto::GM_SPECTATOR || game.players.players[&id].dead {
        return;
    }
    // Block.onBlockActivated: the crafting table, chest and furnace (a sneaking player with an
    // item in hand skips it, which windows.rs checks).
    if windows::use_on(game, id, m) || beds::use_on(game, id, m) || doors::use_on(game, id, m) || jukebox::use_on(game, id, m) {
        return;
    }
    let Some(state) = game.sessions.get(&id).and_then(|s| s.state) else { return };
    let at = (m.pos[0], m.pos[1], m.pos[2]);
    let (dx, dy, dz) = (state.pos[0] - (at.0 as f64 + 0.5), state.pos[1] - (at.1 as f64 + 0.5), state.pos[2] - (at.2 as f64 + 0.5));
    if dx * dx + dy * dy + dz * dz < REACH_SQ {
        let held = held(game, id, m.hand);
        if let Some((_, f)) = USES.iter().find(|(test, _)| test(&held)) {
            // ItemStack.onItemUse: the creative player keeps the stack.
            if let Used::Success { consume: true } = f(game, id, m, held) {
                if players::mode(game, id) != proto::GM_CREATIVE {
                    if let Some(s) = slot(game, id, m.hand) {
                        s.shrink(1);
                    }
                }
            }
        }
    }
    resend(game, id, m);
}

pub(super) fn slot(game: &mut Game, id: u32, hand: u8) -> Option<&mut ItemStack> {
    let inv = &mut game.players.players.get_mut(&id)?.inv;
    let cur = inv.current_item;
    Some(if hand == 0 { &mut inv.main[cur] } else { &mut inv.off_hand[0] })
}

fn held(game: &mut Game, id: u32, hand: u8) -> ItemStack {
    slot(game, id, hand).map_or(ItemStack::EMPTY, |s| *s)
}

/// `NetHandlerPlayServer.processTryUseItemOnBlock` sends the clicked block and the one beside it
/// back whatever happened, so a client that guessed a placement is corrected. The used hand's
/// slot goes back too: the viewer shrinks its stack when it places (as `ItemBlock.onItemUse`
/// does on the client), and a refused placement leaves no server change for windows.rs's diff
/// to send.
fn resend(game: &mut Game, id: u32, m: &UseOn) {
    let at = (m.pos[0], m.pos[1], m.pos[2]);
    let Some(face) = Facing::VALUES.get(m.face as usize) else { return };
    let (ox, oy, oz) = face.offset();
    let beside = (at.0 + ox, at.1 + oy, at.2 + oz);
    let mut cells = vec![at, beside, (beside.0, beside.1 + 1, beside.2)];
    cells.dedup();
    let changes: Vec<proto::BlockChange> = cells.into_iter().map(|c| (c, terrain::state(game, c))).collect();
    net::send(game, id, proto::S_BLOCKS, &proto::encode_blocks(&changes));
    let Some(cur) = game.players.players.get(&id).map(|p| p.inv.current_item) else { return };
    let index = if m.hand == 0 { 36 + cur as i16 } else { 45 };
    let stack = held(game, id, m.hand);
    net::send(game, id, proto::S_SLOTS, &proto::WindowSlots { window: 0, slots: vec![(index, windows::wire(&stack))] }.encode());
}

/// `World.checkNoEntityCollision` for a placement: no living player (not a spectator) and no mob
/// overlaps `bb`.
fn no_entity_in(game: &Game, bb: &sim::collision::Aabb) -> bool {
    for (&pid, s) in &game.sessions {
        let Some(st) = s.state else { continue };
        let alive = game.players.players.get(&pid).is_some_and(|p| !p.dead);
        if alive && players::mode(game, pid) != proto::GM_SPECTATOR && players::player_bb(st.pos).intersects(bb) {
            return false;
        }
    }
    !game.mobs.mobs.iter().any(|mob| !mob.dead && mob.body.bb.intersects(bb))
}

/// `ItemBlock.onItemUse`: the block goes into the replaceable cell clicked, else the one beside
/// it, if `World.mayPlace` allows. The state is the one the viewer worked out from the click
/// (`UseOn.state`: log axis, slab half, torch and vine facing, chest facing), checked to be the
/// held block's. 0 means the viewer placed nothing (it used a door instead, say) and the item
/// does nothing.
/// DROPPED: `ItemSlab` and `ItemSnow` growing the block clicked (the viewer shares those as
/// `V_BLOCKS`), `getStateForPlacement` of its own (the viewer's is the only one ported),
/// `setTileEntityNBT`.
fn item_block_use(game: &mut Game, _pid: u32, m: &UseOn, stack: ItemStack) -> Used {
    let Some(block) = placed_block(&stack) else { return Used::No };
    let Some(face) = Facing::VALUES.get(m.face as usize).copied() else { return Used::No };
    if m.state == 0 || m.state >> 4 != block {
        return Used::No;
    }
    let at = (m.pos[0], m.pos[1], m.pos[2]);
    let q = {
        let w = Blocks(game);
        if is_replaceable(&w, at) {
            at
        } else {
            let (ox, oy, oz) = face.offset();
            (at.0 + ox, at.1 + oy, at.2 + oz)
        }
    };
    if q.1 < 0 || q.1 > 255 {
        return Used::No;
    }
    let ok = may_place_among(&Blocks(game), |bb| no_entity_in(game, &bb), block, q, face);
    if !ok {
        return Used::No;
    }
    terrain::set_block(game, q, m.state);
    // BlockDoublePlant.onBlockPlacedBy: the upper half, facing north.
    if block == id::DOUBLE_PLANT {
        terrain::set_block(game, (q.0, q.1 + 1, q.2), worldgen::blocks::state(id::DOUBLE_PLANT, 10));
    }
    Used::Success { consume: true }
}

/// `ItemMonsterPlacer.onItemUse`: the egg's mob stands in the block beside the face clicked
/// (half a block up on a fence), with a random facing.
/// DROPPED: a mob spawner clicked (its spawn data), the custom name, and `playLivingSound`. An
/// egg of a kind the server has no mob for does nothing.
fn spawn_egg_use(game: &mut Game, _id: u32, m: &UseOn, stack: ItemStack) -> Used {
    let Tag::Entity(i) = stack.tag else { return Used::No };
    let Some(egg) = ::items::creative::egg(i) else { return Used::No };
    let Some(face) = Facing::VALUES.get(m.face as usize).copied() else { return Used::No };
    let Some(def) = mobs::def_by_name(egg.entity) else { return Used::No };
    let at = (m.pos[0], m.pos[1], m.pos[2]);
    let (ox, oy, oz) = face.offset();
    let pos = (at.0 + ox, at.1 + oy, at.2 + oz);
    let clicked = terrain::state(game, at) >> 4;
    let fence = matches!(
        clicked,
        bid::OAK_FENCE | bid::NETHER_BRICK_FENCE | bid::SPRUCE_FENCE | bid::BIRCH_FENCE | bid::JUNGLE_FENCE | bid::DARK_OAK_FENCE | bid::ACACIA_FENCE
    );
    let d0 = if face == Facing::Up && fence { 0.5 } else { 0.0 };
    // ItemMonsterPlacer.spawnCreature: MathHelper.wrapDegrees(world.rand.nextFloat() * 360).
    let mut yaw = game.mobs.rand.next_float() * 360.0 % 360.0;
    if yaw >= 180.0 {
        yaw -= 360.0;
    }
    if !sim_loaded(game, pos) {
        return Used::No;
    }
    mobs::spawn_initial(game, def, [pos.0 as f64 + 0.5, pos.1 as f64 + d0, pos.2 as f64 + 0.5], yaw);
    Used::Success { consume: true }
}

fn sim_loaded(game: &Game, pos: (i32, i32, i32)) -> bool {
    sim::collision::BlockAccess::loaded(&Blocks(game), pos.0, pos.2)
}
