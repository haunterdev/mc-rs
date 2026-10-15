//! The creative player's hands on the world: `Minecraft.clickMouse`, `rightClickMouse`,
//! `middleClickMouse` and `sendClickBlockToController`, the creative half of
//! `PlayerControllerMP`, and the `ItemBlock` placement rules behind them (`World.mayPlace`, the
//! blocks' `canPlaceBlockAt`, `getStateForPlacement`, and the slab, snow and lily pad items).
//!
//! The client guesses each change at once; the populator thread (the server) checks the
//! placements it can and answers with what the world ends up with (`Streamer::edit`).

use bevy::math::DVec3;
use bevy::prelude::*;
use items::{ItemStack, Kind as ItemKind};
use sim::placement::{can_place_block_at, clear_of, double_plant_variant, is_replaceable, may_place};
use worldgen::blocks::{id, state, WATERLILY};
use worldgen::populate::behavior::{torch_can_place_at, torch_meta, vine_bit};
use worldgen::populate::state as st;
use worldgen::populate::Facing;

use crate::blockstates::{double_slab, half_slab};
use crate::collision::{collision_box, floor, Aabb, BlockAccess};
use crate::hand::Hand;
use crate::hotbar::{block_stack, placed_block, Inventory};
use crate::player::{look, Player};
use crate::populator::Edit;
use crate::region::{props, Mat};
use crate::stream::Streamer;
use crate::target::{mouse_over_entities, offset, ray_trace_blocks, BlockPos, EntityHit, Hit, Kind};

mod door;

use crate::survival::mining::{dig_context, Hold, Mining};

/// The client's world: what it reads, and the changes it guesses.
pub trait World: BlockAccess {
    fn edit(&mut self, edits: Vec<Edit>);
    /// A survival placement: shown at once, but the server hears of it through `V_USE_ON`.
    fn edit_quiet(&mut self, edits: Vec<Edit>) {
        self.edit(edits)
    }
}

impl World for Streamer {
    fn edit(&mut self, edits: Vec<Edit>) {
        Streamer::edit(self, edits)
    }
    fn edit_quiet(&mut self, edits: Vec<Edit>) {
        Streamer::edit_quiet(self, edits)
    }
}

/// A world that only reads: the edits a placement makes are kept to be sent on afterwards.
struct Recorder<'a, W: World> {
    w: &'a W,
    edits: Vec<Edit>,
}

impl<W: World> BlockAccess for Recorder<'_, W> {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        self.w.state(x, y, z)
    }
    fn loaded(&self, x: i32, z: i32) -> bool {
        self.w.loaded(x, z)
    }
}

impl<W: World> World for Recorder<'_, W> {
    fn edit(&mut self, edits: Vec<Edit>) {
        self.edits.extend(edits);
    }
}

/// `Minecraft.objectMouseOver`, `rightClickDelayTimer`, `PlayerControllerMP.blockHitDelay`, and
/// the use and pick-block presses not yet handled (`KeyBinding.pressTime`). The attack presses
/// are `Hand::clicks`.
#[derive(Resource, Default)]
pub struct Controller {
    pub over: Option<Hit>,
    /// `objectMouseOver` when it is an ENTITY: the mob or player under the crosshair, in which
    /// case `over` is None (survival/attack.rs sends the attack).
    pub entity: Option<EntityHit>,
    /// The boxes `getMouseOver` may point at, refreshed each frame (survival/attack.rs).
    pub entities: Vec<(u32, Aabb)>,
    /// `PlayerControllerMP.attackEntity` calls made, waiting for the `V_ATTACK` send.
    pub attacks: Vec<u32>,
    /// `PlayerControllerMP.interactWithEntity` calls made, waiting for the `V_INTERACT` send, each
    /// with the hand it used (`Minecraft.rightClickMouse`'s `enumhand`).
    pub interacts: Vec<(u32, u8)>,
    pub right_click_delay: i32,
    pub block_hit_delay: i32,
    pub uses: u32,
    pub picks: u32,
    /// `PlayerControllerMP`'s survival fields and what they want sent (survival/mining.rs).
    pub mining: Mining,
    /// The sign just placed, for the edit screen (`EntityPlayer.openEditSign`, sign.rs).
    pub sign: Option<BlockPos>,
    /// The crafting table, chest or furnace just used: survival/gui/window.rs sends `V_USE_ON` and opens its screen.
    pub table: Option<Hit>,
    /// The `CPacketPlayerTryUseItemOnBlock`s of a survival session, waiting for the `V_USE_ON`
    /// send (survival/gui/window.rs): the block clicked, the state the client placed (0 none) and
    /// the hand (`Minecraft.rightClickMouse`'s `enumhand`).
    pub use_on: Vec<(Hit, u16, u8)>,
}

/// `EnumActionResult`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Success,
    Pass,
    Fail,
}

fn get(w: &impl BlockAccess, p: BlockPos) -> u16 {
    w.state(p.0, p.1, p.2)
}

fn up(p: BlockPos) -> BlockPos {
    (p.0, p.1 + 1, p.2)
}

fn down(p: BlockPos) -> BlockPos {
    (p.0, p.1 - 1, p.2)
}

fn edit(at: BlockPos, s: u16, flags: i32, place: bool) -> Edit {
    Edit { at, s, flags, place }
}

fn air(s: u16) -> bool {
    props(s).mat == Mat::Air
}


/// `ItemBlock.canPlaceBlockOnSide`, with `ItemSlab`'s: the check the client makes before it
/// sends a placement at all.
fn item_can_place_block_on_side(w: &impl BlockAccess, bb: &Aabb, stack: ItemStack, p: BlockPos, side: Facing) -> bool {
    let (block, meta) = block_meta(&stack);
    if double_slab(block).is_some() {
        let variant = meta & 7;
        let s = get(w, p);
        if s >> 4 == block {
            let top = s & 8 != 0;
            if ((side == Facing::Up && !top) || (side == Facing::Down && top)) && s & 7 == variant {
                return true;
            }
        }
        let s1 = get(w, offset(p, side));
        if s1 >> 4 == block && s1 & 7 == variant {
            return true;
        }
    }
    let (mut p, mut side) = (p, side);
    if get(w, p) >> 4 == id::SNOW_LAYER {
        side = Facing::Up;
    } else if !is_replaceable(w, p) {
        p = offset(p, side);
    }
    may_place(w, bb, block, p, side)
}

/// The block a block item places and the stack's metadata. Only stacks [`placed_block`] knows
/// reach the placement code.
fn block_meta(stack: &ItemStack) -> (u16, u16) {
    placed_block(stack).unwrap_or((id::AIR, 0))
}

/// `Entity.getHorizontalFacing`
fn horizontal_facing(yaw: f32) -> Facing {
    Facing::BY_HORIZONTAL[(floor((yaw * 4.0 / 360.0) as f64 + 0.5) & 3) as usize]
}

/// `ItemBlock.getMetadata` and `Block.getStateForPlacement`, with the facing `onBlockPlacedBy`
/// gives a chest.
/// DROPPED: a chest joining the chest beside it.
fn state_for_placement(stack: ItemStack, facing: Facing, hit_y: f32, yaw: f32) -> u16 {
    let (b, m) = block_meta(&stack);
    let meta = match b {
        // BlockLog.LOG_AXIS from the clicked face's axis: Y 0, X 4, Z 8.
        id::LOG | id::LOG2 => {
            (m & 3)
                | match facing {
                    Facing::Down | Facing::Up => 0,
                    Facing::West | Facing::East => 4,
                    Facing::North | Facing::South => 8,
                }
        }
        id::PUMPKIN => horizontal_facing(yaw).opposite().horizontal_index() as u16,
        // BlockSlab: the top half from below, or from the upper half of a side.
        id::STONE_SLAB | id::WOODEN_SLAB | id::STONE_SLAB2 | id::PURPUR_SLAB => (m & 7) | if facing == Facing::Down || (facing != Facing::Up && hit_y > 0.5) { 8 } else { 0 },
        id::VINE => {
            if facing.is_horizontal() {
                vine_bit(facing.opposite())
            } else {
                0
            }
        }
        id::CHEST => horizontal_facing(yaw).opposite() as u16,
        // The default state: every side the cap.
        id::BROWN_MUSHROOM_BLOCK | id::RED_MUSHROOM_BLOCK => 14,
        // ItemLeaves: placed leaves never decay.
        id::LEAVES | id::LEAVES2 => (m & 3) | 4,
        _ => m,
    };
    state(b, meta)
}

/// `BlockTorch.getStateForPlacement`: the clicked face if the torch can hang there, else the
/// first horizontal facing that can, else standing.
fn torch_for_placement(w: &impl BlockAccess, p: BlockPos, side: Facing) -> u16 {
    let fits = |f: Facing| torch_can_place_at(get(w, offset(p, f.opposite())), f);
    let f = if fits(side) { side } else { Facing::HORIZONTAL.into_iter().find(|&f| fits(f)).unwrap_or(Facing::Up) };
    state(id::TORCH, torch_meta(f))
}

/// `ItemBlock.onItemUse`, and `ItemBlockSpecial`'s for reeds, which comes to the same here.
/// DROPPED: the build height check `NetHandlerPlayServer` makes; the populator ignores what is
/// out of the world.
fn item_block_use(w: &mut impl World, player: &Player, stack: ItemStack, p: BlockPos, side: Facing, hit_y: f32) -> Action {
    let block = block_meta(&stack).0;
    let q = if is_replaceable(w, p) { p } else { offset(p, side) };
    if !may_place(w, &player.bb, block, q, side) {
        return Action::Fail;
    }
    let s = if block == id::TORCH { torch_for_placement(w, q, side) } else { state_for_placement(stack, side, hit_y, player.yaw) };
    let mut edits = vec![edit(q, s, 11, true)];
    // BlockDoublePlant.onBlockPlacedBy: the upper half, facing north.
    if block == id::DOUBLE_PLANT {
        edits.push(edit(up(q), state(id::DOUBLE_PLANT, 10), 2, false));
    }
    w.edit(edits);
    Action::Success
}

/// The `setBlockState(pos, state, flags)` the slab and snow items make when the grown block's
/// collision box is clear of the player.
fn grow(w: &mut impl World, player: &Player, p: BlockPos, s: u16, flags: i32) -> bool {
    let clear = collision_box(s).is_some_and(|b| clear_of(b, p, &player.bb));
    if clear {
        w.edit(vec![edit(p, s, flags, false)]);
    }
    clear
}

/// `ItemStack.onItemUse` for the block items: `ItemSlab` and `ItemSnow` grow the block clicked,
/// everything else is `item_block_use`.
fn on_item_use(w: &mut impl World, player: &Player, stack: ItemStack, p: BlockPos, side: Facing, hit_y: f32) -> Action {
    let (block, meta) = block_meta(&stack);
    if let Some(double) = double_slab(block) {
        let variant = meta & 7;
        let s = get(w, p);
        if s >> 4 == block {
            let top = s & 8 != 0;
            if ((side == Facing::Up && !top) || (side == Facing::Down && top)) && s & 7 == variant {
                grow(w, player, p, state(double, variant), 11);
                return Action::Success;
            }
        }
        // ItemSlab.tryPlace
        let q = offset(p, side);
        let s1 = get(w, q);
        if s1 >> 4 == block && s1 & 7 == variant {
            grow(w, player, q, state(double, variant), 11);
            return Action::Success;
        }
        return item_block_use(w, player, stack, p, side, hit_y);
    }
    match block {
        id::SNOW_LAYER => {
            let mut q = p;
            let mut s = get(w, p);
            if (side != Facing::Up || s >> 4 != id::SNOW_LAYER) && !is_replaceable(w, p) {
                q = offset(p, side);
                s = get(w, q);
            }
            // One layer more, up to eight.
            if s >> 4 == id::SNOW_LAYER && s & 7 < 7 && grow(w, player, q, state(id::SNOW_LAYER, (s & 7) + 1), 10) {
                return Action::Success;
            }
            item_block_use(w, player, stack, p, side, hit_y)
        }
        _ => item_block_use(w, player, stack, p, side, hit_y),
    }
}

/// `ItemSign.onItemUse`: a standing sign facing the player on top of a block, a wall sign on its
/// side, and its edit screen (`openEditSign`), which sign.rs opens from `opened`.
/// DROPPED: `canPlayerEdit`, true for a creative player; `ItemBlock.setTileEntityNBT`, as no
/// stack carries tile data; the placed-block trigger; and `shrink`, as creative keeps the stack.
fn item_sign_use(w: &mut impl World, player: &Player, p: BlockPos, facing: Facing, opened: &mut Option<BlockPos>) -> Action {
    let s = get(w, p);
    let flag = is_replaceable(w, p);
    if facing != Facing::Down && (st::mat_solid(s) || flag) && (!flag || facing == Facing::Up) {
        let q = offset(p, facing);
        if can_place_block_at(w, q, id::STANDING_SIGN) {
            let q = if flag { down(q) } else { q };
            let s = if facing == Facing::Up {
                let i = floor(((player.yaw + 180.0) * 16.0 / 360.0) as f64 + 0.5) & 15;
                state(id::STANDING_SIGN, i as u16)
            } else {
                state(id::WALL_SIGN, facing as u16)
            };
            w.edit(vec![edit(q, s, 11, true)]);
            *opened = Some(q);
            return Action::Success;
        }
    }
    Action::Fail
}

/// `PlayerControllerMP.processRightClickBlock` in creative.
/// DROPPED: every `onBlockActivated` but the chest's, which on the client only answers true (its
/// screen is not ported), the door's and the crafting table's.
///
/// In a survival session `placed` is Some: a plain block placement is shown but not shared as
/// `V_BLOCKS`, and the state it placed comes back in `placed` for the `V_USE_ON`.
fn process_right_click_block(
    w: &mut impl World,
    player: &Player,
    held: ItemStack,
    hit: Hit,
    opened: &mut Option<BlockPos>,
    table: &mut Option<Hit>,
    placed: Option<&mut Option<u16>>,
) -> Action {
    let p = hit.pos;
    // BlockWorkbench.onBlockActivated opens the crafting screen through the server; a sneaking
    // player with an item in hand uses the item instead.
    if (!player.is_sneaking() || held.is_empty()) && get(w, p) >> 4 == items::ids::block::CRAFTING_TABLE {
        *table = Some(hit);
        return Action::Success;
    }
    // BlockChest and BlockFurnace.onBlockActivated open their screens through the server too.
    if (!player.is_sneaking() || held.is_empty()) && (get(w, p) >> 4 == id::CHEST || matches!(get(w, p) >> 4, items::ids::block::FURNACE | items::ids::block::LIT_FURNACE)) {
        *table = Some(hit);
        return Action::Success;
    }
    // BlockJukebox.onBlockActivated ejects a disc, ItemRecord.onItemUse inserts one; the server
    // does both from the V_USE_ON and the block change comes back.
    let state = get(w, p);
    if state >> 4 == items::ids::block::JUKEBOX {
        let record = !held.is_empty() && items::tooltip::record_name(held.item).is_some();
        if state & 15 > 0 {
            if !player.is_sneaking() || held.is_empty() {
                return Action::Success;
            }
        } else if record {
            return Action::Success;
        }
    }
    let hit_y = (hit.vec.y - p.1 as f64) as f32;
    // In a survival session the server swings the door from the V_USE_ON; the swing is shown at
    // once but not shared as V_BLOCKS.
    let survival = placed.is_some();
    let activated = (!player.is_sneaking() || held.is_empty())
        && (get(w, p) >> 4 == id::CHEST
            || if survival {
                let mut rec = Recorder { w: &*w, edits: Vec::new() };
                let used = door::activate(&mut rec, p);
                let edits = rec.edits;
                if used {
                    w.edit_quiet(edits);
                }
                used
            } else {
                door::activate(w, p)
            });
    if !activated {
        // Reeds are an ItemBlockSpecial, not an ItemBlock, and skip the check.
        if !held.is_empty() && matches!(held.def().kind, ItemKind::Block { .. }) && !item_can_place_block_on_side(w, &player.bb, held, p, hit.face) {
            return Action::Fail;
        }
    }
    if activated {
        return Action::Success;
    }
    if held.is_empty() {
        return Action::Pass;
    }
    match placed_block(&held) {
        Some((block, _)) => match placed {
            Some(out) => {
                let mut rec = Recorder { w: &*w, edits: Vec::new() };
                let action = on_item_use(&mut rec, player, held, p, hit.face, hit_y);
                let edits = rec.edits;
                let first = edits.iter().map(|e| e.s).find(|&s| s != 0);
                if first.is_some_and(|s| s >> 4 == block) {
                    *out = first;
                    w.edit_quiet(edits);
                } else if !edits.is_empty() {
                    // A slab or snow layer grown: shared as before.
                    w.edit(edits);
                }
                action
            }
            None => on_item_use(w, player, held, p, hit.face, hit_y),
        },
        // ItemMonsterPlacer.onItemUse is SUCCESS on the client; the server spawns the mob.
        None if placed.is_some() && held.item == items::ids::item::SPAWN_EGG => Action::Success,
        None if held.item == items::ids::item::SIGN => item_sign_use(w, player, p, hit.face, opened),
        None => match door::door_block(held.item) {
            // The server places the door from the V_USE_ON; a survival player sees it at once.
            Some(b) if survival => {
                let mut rec = Recorder { w: &*w, edits: Vec::new() };
                let action = door::item_door_use(&mut rec, player, b, hit);
                let edits = rec.edits;
                if !edits.is_empty() {
                    w.edit_quiet(edits);
                }
                action
            }
            Some(b) => door::item_door_use(w, player, b, hit),
            // DROPPED: Item.onItemUse of the other items that place no block.
            None => Action::Pass,
        },
    }
}

/// `ItemLilyPad.onItemRightClick`: `Item.rayTrace` with liquids, and the pad on the still water
/// it finds.
fn use_lily_pad(w: &mut impl World, player: &Player) -> Action {
    let start = player.pos + DVec3::Y * player.eye_height() as f64;
    let l = look(player.pitch, player.yaw).as_dvec3();
    match ray_trace_blocks(w, start, start + DVec3::new(l.x * 5.0, l.y * 5.0, l.z * 5.0), true, false) {
        None => Action::Pass,
        Some(h) if h.kind == Kind::Block => {
            let s = get(w, h.pos);
            if props(s).mat == Mat::Water && s & 15 == 0 && get(w, up(h.pos)) == 0 {
                w.edit(vec![edit(up(h.pos), WATERLILY, 11, false)]);
                Action::Success
            } else {
                Action::Fail
            }
        }
        Some(_) => Action::Fail,
    }
}

/// `PlayerControllerMP.processRightClick`: `Item.onItemRightClick`, which only the lily pad among
/// the block items overrides.
fn process_right_click(w: &mut impl World, player: &Player, stack: ItemStack) -> Action {
    if placed_block(&stack).is_some_and(|(b, _)| b == id::WATERLILY) {
        use_lily_pad(w, player)
    } else {
        Action::Pass
    }
}

/// `Minecraft.rightClickMouse`: the two hands in order (`for (EnumHand hand : EnumHand.values())`),
/// MAIN then OFF. The first hand whose use succeeds ends the click; a main hand that returns
/// `EnumActionResult.PASS` falls through to the off hand.
///
/// An ENTITY under the crosshair goes to `PlayerControllerMP.interactWithEntity`: the `V_INTERACT`
/// is queued and the click ends there, with no arm swing (vanilla swings only for a block use) and
/// no block use or placement.
/// DROPPED: the `INTERACT_AT` packet (proto has none), and the client's own result
/// (`Entity.applyPlayerInteraction` is PASS, `EntityPlayer.interactOn` runs `processInteract` and
/// `itemInteractionForEntity` on the client copy): a PASS would fall through to the held item's
/// use, here the server's answer is the only one, so the click always ends.
fn right_click_mouse(w: &mut impl World, ctl: &mut Controller, hand: &mut Hand, inv: &mut Inventory, player: &Player) {
    ctl.right_click_delay = 4;
    if let Some(target) = ctl.entity {
        ctl.interacts.push((target.id, proto::HAND_MAIN));
        return;
    }
    for enum_hand in [proto::HAND_MAIN, proto::HAND_OFF] {
        let held = if enum_hand == proto::HAND_OFF { inv.get_stack_in_slot(40) } else { inv.get_current_item() };
        if right_click_hand(w, ctl, hand, inv, player, enum_hand, held) {
            return;
        }
    }
}

/// One iteration of `Minecraft.rightClickMouse`'s hand loop for `enum_hand`; `true` when the click
/// ends there. `PlayerControllerMP.processRightClickBlock` sends the packet for every click, then
/// runs the client's own `onBlockActivated` and `onItemUse`, so the block case runs for each hand
/// until one succeeds. The `resetEquippedProgress` of an air use fires for the hand that used.
fn right_click_hand(w: &mut impl World, ctl: &mut Controller, hand: &mut Hand, inv: &mut Inventory, player: &Player, enum_hand: u8, held: ItemStack) -> bool {
    if let Some(h) = ctl.over.filter(|h| h.kind == Kind::Block) {
        if !air(get(w, h.pos)) {
            let survival = player.game_mode == proto::GM_SURVIVAL;
            let mut placed = None;
            let earlier = ctl.table.take();
            let action = process_right_click_block(w, player, held, h, &mut ctl.sign, &mut ctl.table, survival.then_some(&mut placed));
            let opened = ctl.table.is_some();
            if ctl.table.is_none() {
                ctl.table = earlier;
            }
            // A table, chest or furnace is sent by the window code, with its screen.
            // A jukebox is the server's in every mode (its disc is a tile).
            if (survival || get(w, h.pos) >> 4 == items::ids::block::JUKEBOX) && !opened {
                ctl.use_on.push((h, placed.unwrap_or(0), enum_hand));
            }
            if action == Action::Success {
                hand.swing_arm(enum_hand);
                // ItemBlock.onItemUse shrinks the client's own stack in survival, so the last
                // block leaves the hand on the click; use_on.rs sends the slot back either way.
                if placed.is_some_and(|s| s != 0) {
                    if enum_hand == proto::HAND_OFF {
                        inv.off_hand[0].shrink(1);
                    } else {
                        let cur = inv.current_item;
                        inv.main[cur].shrink(1);
                    }
                }
                // CHANGED: Minecraft.rightClickMouse calls resetEquippedProgress here when the
                // count changed or in creative. xy, playing 1.12.2 beside it (feedback round 4):
                // "the hand still does the durability jump thing which isnt in 1.12.2", so a
                // block use never dips the hand.
                return true;
            }
        }
    }
    if !held.is_empty() && process_right_click(w, player, held) == Action::Success {
        hand.reset_equipped(enum_hand);
        return true;
    }
    false
}

/// `PlayerControllerMP.onPlayerDestroyBlock`, with `BlockDoublePlant.onBlockHarvested` and
/// `BlockDoor.onBlockHarvested` taking the other half. `quiet` is a survival break: the server
/// breaks the block itself from the running dig (`V_DIG`), so the guess is shown but not shared
/// as `V_BLOCKS` (like a survival placement, [`World::edit_quiet`]).
/// DROPPED: the sword that cannot break blocks in creative, and the break particles and sound.
pub(crate) fn on_player_destroy_block(w: &mut impl World, p: BlockPos, quiet: bool) {
    let s = get(w, p);
    if air(s) {
        return;
    }
    let mut edits = Vec::new();
    if s >> 4 == id::DOUBLE_PLANT {
        if s & 8 != 0 {
            if get(w, down(p)) >> 4 == id::DOUBLE_PLANT {
                edits.push(edit(down(p), 0, 3, false));
            }
        } else if get(w, up(p)) >> 4 == id::DOUBLE_PLANT {
            edits.push(edit(up(p), 0, 2, false));
        }
    } else if worldgen::blocks::is_door(s >> 4) {
        door::harvest_edits(w, p, s, &mut edits);
    }
    edits.push(edit(p, 0, 11, false));
    if quiet {
        w.edit_quiet(edits);
    } else {
        w.edit(edits);
    }
}

/// `PlayerControllerMP.clickBlockCreative`: the fire on the clicked face goes out first
/// (`World.extinguishFire`), else the block breaks.
fn click_block_creative(w: &mut impl World, p: BlockPos, face: Facing) {
    let f = offset(p, face);
    if get(w, f) >> 4 == id::FIRE {
        w.edit(vec![edit(f, 0, 3, false)]);
    } else {
        on_player_destroy_block(w, p, false);
    }
}

/// `PlayerControllerMP.attackEntity` for `Minecraft.clickMouse`'s ENTITY case: the `V_ATTACK` is
/// queued, and `attackTargetEntityWithCurrentItem` and `resetCooldown` reset the cooldown, which
/// is all the client side of it does: its `attackEntityFrom` answers false on a remote world, so
/// the knockback, the `setSprinting(false)`, the crit and the sounds stay with the server.
/// DROPPED: `syncCurrentPlayItem` (`V_HELD` is the hotbar's), the spectator check.
fn attack_entity(ctl: &mut Controller, hand: &mut Hand, target: EntityHit) {
    ctl.attacks.push(target.id);
    hand.reset_cooldown();
}

/// `Minecraft.clickMouse` with `PlayerControllerMP.clickBlock` in creative.
fn click_mouse(w: &mut impl World, ctl: &mut Controller, hand: &mut Hand) {
    if hand.left_click_counter > 0 {
        return;
    }
    if let Some(target) = ctl.entity {
        attack_entity(ctl, hand, target);
        hand.swing_arm(proto::HAND_MAIN);
        return;
    }
    let Some(h) = ctl.over else { return };
    if h.kind == Kind::Block && !air(get(w, h.pos)) {
        click_block_creative(w, h.pos, h.face);
        ctl.block_hit_delay = 5;
    } else {
        hand.reset_cooldown();
    }
    hand.swing_arm(proto::HAND_MAIN);
}

/// `Minecraft.sendClickBlockToController` with `PlayerControllerMP.onPlayerDamageBlock` in
/// creative: a held attack breaks a block every sixth tick.
/// DROPPED: the hit particles, and `resetBlockRemoving`, which has no progress to reset in creative.
fn send_click_block_to_controller(w: &mut impl World, ctl: &mut Controller, hand: &mut Hand, attack_held: bool) {
    if !attack_held {
        hand.left_click_counter = 0;
    }
    if hand.left_click_counter > 0 || !attack_held {
        return;
    }
    let Some(h) = ctl.over.filter(|h| h.kind == Kind::Block) else { return };
    if air(get(w, h.pos)) {
        return;
    }
    if ctl.block_hit_delay > 0 {
        ctl.block_hit_delay -= 1;
    } else {
        ctl.block_hit_delay = 5;
        click_block_creative(w, h.pos, h.face);
    }
    hand.swing_arm(proto::HAND_MAIN);
}

/// `Minecraft.clickMouse` with `PlayerControllerMP.clickBlock` in survival: the dig starts (or
/// the block breaks at once), and a click on air or nothing costs ten ticks of cooldown
/// (`leftClickCounter`). An entity under the crosshair is [`attack_entity`] instead, with no
/// `leftClickCounter`.
fn click_mouse_survival(w: &mut impl World, ctl: &mut Controller, hand: &mut Hand, hold: &Hold) {
    if hand.left_click_counter > 0 {
        return;
    }
    if let Some(target) = ctl.entity {
        attack_entity(ctl, hand, target);
        hand.swing_arm(proto::HAND_MAIN);
        return;
    }
    match ctl.over.filter(|h| h.kind == Kind::Block && !air(get(w, h.pos))) {
        Some(h) => {
            ctl.mining.click_block(w, h.pos, h.face, hold);
        }
        None => {
            hand.left_click_counter = 10;
            hand.reset_cooldown();
        }
    }
    hand.swing_arm(proto::HAND_MAIN);
}

/// `Minecraft.sendClickBlockToController` with `PlayerControllerMP.onPlayerDamageBlock` in
/// survival: a held attack on a block adds to its damage every tick, anything else lets go.
/// DROPPED: the hit particles, and `isHandActive` (no item is being used).
fn send_click_block_to_controller_survival(w: &mut impl World, ctl: &mut Controller, hand: &mut Hand, attack_held: bool, hold: &Hold) {
    if !attack_held {
        hand.left_click_counter = 0;
    }
    if hand.left_click_counter > 0 {
        return;
    }
    match ctl.over.filter(|h| attack_held && h.kind == Kind::Block) {
        Some(h) => {
            if !air(get(w, h.pos)) && ctl.mining.on_player_damage_block(w, &mut ctl.block_hit_delay, h.pos, h.face, hold) {
                hand.swing_arm(proto::HAND_MAIN);
            }
        }
        None => {
            if ctl.mining.is_hitting_block {
                ctl.mining.reset_block_removing();
                hand.reset_cooldown();
            }
        }
    }
}

/// `Block.getItem`: the stack the middle click picks, as a block and metadata.
/// DROPPED: the item-only results (cocoa beans) and the stored tile entity data. The sign's and
/// the door's items are `middle_click_mouse`'s.
fn pick(w: &impl BlockAccess, s: u16, p: BlockPos) -> Option<(u16, u16)> {
    let (b, m) = (s >> 4, s & 15);
    Some(match b {
        id::AIR | id::FLOWING_WATER | id::WATER | id::FLOWING_LAVA | id::LAVA | id::FIRE | id::MOB_SPAWNER | id::PORTAL | id::FROSTED_ICE => {
            return None
        }
        id::COCOA | id::STANDING_SIGN | id::WALL_SIGN | id::PISTON_HEAD => return None,
        _ if worldgen::blocks::is_door(b) => return None,
        // damageDropped: the variant.
        id::STONE
        | id::DIRT
        | id::PLANKS
        | id::SAND
        | id::SANDSTONE
        | id::RED_SANDSTONE
        | id::TALLGRASS
        | id::RED_FLOWER
        | id::STONEBRICK
        | id::STAINED_GLASS
        | id::STAINED_HARDENED_CLAY
        | id::MONSTER_EGG => (b, m),
        id::SAPLING => (b, m & 7),
        id::LOG | id::LOG2 | id::LEAVES | id::LEAVES2 => (b, m & 3),
        // A double slab picks its half.
        id::STONE_SLAB | id::WOODEN_SLAB | id::STONE_SLAB2 | id::PURPUR_SLAB => (b, m & 7),
        _ if half_slab(b).is_some() => (half_slab(b).unwrap(), m & 7),
        id::DOUBLE_PLANT => (b, double_plant_variant(w, s, p)),
        // Lit redstone ore.
        74 => (id::REDSTONE_ORE, 0),
        id::BROWN_MUSHROOM_BLOCK => (id::BROWN_MUSHROOM, 0),
        id::RED_MUSHROOM_BLOCK => (id::RED_MUSHROOM, 0),
        id::FARMLAND => (id::DIRT, 0),
        _ => (b, 0),
    })
}

/// [`pick`] with nothing around the block, for the hotbar's block keys (`item::block_keys`). Wrong
/// for a double plant's upper half, which reads the half below.
#[cfg(test)]
pub fn picked_item(s: u16) -> Option<(u16, u16)> {
    struct NoWorld;
    impl BlockAccess for NoWorld {
        fn state(&self, _: i32, _: i32, _: i32) -> u16 {
            0
        }
        fn loaded(&self, _: i32, _: i32) -> bool {
            true
        }
    }
    pick(&NoWorld, s, (0, 64, 0))
}

/// `Minecraft.middleClickMouse` in creative: the pointed block into the hotbar.
fn middle_click_mouse(w: &impl BlockAccess, ctl: &Controller, inv: &mut Inventory) {
    let Some(h) = ctl.over.filter(|h| h.kind == Kind::Block) else { return };
    let s = get(w, h.pos);
    if air(s) {
        return;
    }
    // BlockSign.getItem: the sign item.
    let stack = if matches!(s >> 4, id::STANDING_SIGN | id::WALL_SIGN) {
        ItemStack::of(items::ids::item::SIGN)
    } else if worldgen::blocks::is_door(s >> 4) {
        door::door_item(s >> 4)
    } else {
        let Some((b, m)) = pick(w, s, h.pos) else { return };
        block_stack(b, m)
    };
    if stack.is_empty() {
        return;
    }
    inv.set_picked_item_stack(stack);
    // DROPPED: playerController.sendSlotPacket; no server keeps the creative hotbar.
}

/// One tick of `Minecraft.runTick` up to the world update: the pointed block
/// (`getMouseOver(1.0F)`), the wheel (`runTickMouse`), the hotbar keys and the queued presses
/// (`processKeyBinds`), the held attack, and the hand's equip progress.
#[allow(clippy::too_many_arguments)]
pub fn tick(w: &mut impl World, ctl: &mut Controller, hand: &mut Hand, inv: &mut Inventory, player: &Player, attack_held: bool, use_held: bool) {
    if ctl.right_click_delay > 0 {
        ctl.right_click_delay -= 1;
    }
    (ctl.over, ctl.entity) = mouse_over_entities(w, player, 1.0, &ctl.entities);
    // One InventoryPlayer.changeCurrentItem per wheel event.
    while inv.scroll != 0 {
        let d = inv.scroll.signum();
        inv.change_current_item(d);
        inv.scroll -= d;
    }
    hand.begin_tick();
    hand.set_attack_stats(&inv.get_current_item(), &player.effects);
    // KeyBinding.isPressed takes one press per tick.
    for i in 0..9 {
        if inv.digits[i] > 0 {
            inv.digits[i] -= 1;
            inv.current_item = i;
        }
    }
    let survival = player.game_mode == proto::GM_SURVIVAL;
    let mut dig = dig_context(&*w, player);
    dig.efficiency = items::enchant::get_enchantment_level(items::enchant::id::EFFICIENCY, &inv.get_current_item());
    dig.aqua_affinity = items::enchant::get_max_enchantment_level(items::enchant::id::AQUA_AFFINITY, inv.armor.iter()) > 0;
    let hold = Hold { stack: inv.get_current_item(), dig };
    while hand.clicks > 0 {
        hand.clicks -= 1;
        if survival {
            click_mouse_survival(w, ctl, hand, &hold);
        } else {
            click_mouse(w, ctl, hand);
        }
    }
    while ctl.uses > 0 {
        ctl.uses -= 1;
        right_click_mouse(w, ctl, hand, inv, player);
    }
    while ctl.picks > 0 {
        ctl.picks -= 1;
        middle_click_mouse(w, ctl, inv);
    }
    // Held use repeats every fourth tick.
    if use_held && ctl.right_click_delay == 0 {
        right_click_mouse(w, ctl, hand, inv, player);
    }
    if survival {
        send_click_block_to_controller_survival(w, ctl, hand, attack_held, &hold);
    } else {
        send_click_block_to_controller(w, ctl, hand, attack_held);
    }
    hand.set_attack_stats(&inv.get_current_item(), &player.effects);
    hand.update_main_hand(inv.get_current_item());
    hand.update_equipped(inv.get_current_item(), inv.current_item);
    // ItemRenderer.updateEquippedItem: the off hand is the player inventory's slot 40.
    hand.update_equipped_off(inv.get_stack_in_slot(40));
}

#[cfg(test)]
mod tests;
